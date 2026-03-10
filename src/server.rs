use crate::serve::serve_file;
use seal::state::AppState;
use seal::tls::CertStore;
use crate::url;
use axum::body::Body;
use axum::extract::Host;
use axum::http::{Request, Response, StatusCode};
use axum::response::IntoResponse;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt;

/// SNI-based cert resolver that issues certs on demand via the CertStore.
struct SealCertResolver {
    cert_store: CertStore,
}

impl std::fmt::Debug for SealCertResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealCertResolver").finish()
    }
}

impl ResolvesServerCert for SealCertResolver {
    fn resolve(
        &self,
        client_hello: ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        let hostname = client_hello.server_name()?.to_string();

        let (certs, key) = self.cert_store.resolve(&hostname).ok()?;
        let signing_key = rustls::crypto::ring::sign::any_supported_type(&key).ok()?;

        Some(Arc::new(rustls::sign::CertifiedKey::new(
            certs,
            signing_key,
        )))
    }
}

/// Start the HTTPS server on port 443.
pub async fn run(state: AppState, cert_store: CertStore) -> anyhow::Result<()> {
    let resolver = Arc::new(SealCertResolver { cert_store });

    let tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);

    let acceptor = TlsAcceptor::from(Arc::new(tls_config));

    let addr = SocketAddr::from(([0, 0, 0, 0], 443));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("listening on https://{addr}");

    loop {
        let (stream, _peer) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let state = state.clone();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("TLS handshake failed: {e}");
                    return;
                }
            };

            let io = TokioIo::new(tls_stream);

            let service = hyper::service::service_fn(move |req: Request<hyper::body::Incoming>| {
                let state = state.clone();
                async move { handle_request(state, req).await }
            });

            if let Err(e) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                .serve_connection(io, service)
                .await
            {
                tracing::debug!("connection error: {e}");
            }
        });
    }
}

async fn handle_request(
    state: AppState,
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Body>, std::convert::Infallible> {
    // Extract hostname from the Host header
    let hostname = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .map(|h| h.split(':').next().unwrap_or(h))
        .unwrap_or("")
        .to_string();

    let path = req.uri().path().to_string();

    let response = if url::is_home(&hostname) {
        // Route through the home.seal axum app
        let app = crate::home::router().with_state(state);
        let (parts, body) = req.into_parts();
        let axum_req = Request::from_parts(parts, Body::new(body));
        match app.oneshot(axum_req).await {
            Ok(resp) => resp,
            Err(_) => Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal error"))
                .unwrap(),
        }
    } else if url::is_local_app(&hostname) {
        // Serve local app files
        match url::parse_local_app(&hostname) {
            Some(hash) => {
                let site_dir = state.site_dir(&hash);
                if site_dir.exists() {
                    serve_file(&site_dir, &path).await
                } else {
                    Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::from("App not installed"))
                        .unwrap()
                }
            }
            None => Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Invalid local app URL"))
                .unwrap(),
        }
    } else {
        // For now, registered apps are not yet supported
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "text/html; charset=utf-8")
            .body(Body::from(
                "<html><body><h1>App not installed</h1>\
                 <p>This .seal app is not installed yet. \
                 <a href=\"https://home.seal/\">Go to home.seal</a> to manage your apps.</p>\
                 </body></html>",
            ))
            .unwrap()
    };

    Ok(response)
}
