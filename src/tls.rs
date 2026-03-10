use rcgen::{
    BasicConstraints, CertificateParams, DnType, GeneralSubtree, IsCa, KeyPair, KeyUsagePurpose,
    NameConstraints, SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::path::Path;
use std::sync::{Arc, RwLock};

/// Manages the intermediate CA and issues leaf certificates for *.seal hostnames.
#[derive(Clone)]
pub struct CertStore {
    intermediate_key: Arc<KeyPair>,
    intermediate_cert: Arc<rcgen::Certificate>,
    intermediate_cert_der: Arc<CertificateDer<'static>>,
    cache: Arc<RwLock<std::collections::HashMap<String, CachedCert>>>,
}

struct CachedCert {
    cert_chain: Vec<CertificateDer<'static>>,
    key_der: Vec<u8>,
}

impl CertStore {
    /// Load existing intermediate CA or create the full CA chain from scratch.
    pub fn install(ca_dir: &Path) -> anyhow::Result<Self> {
        let int_key_path = ca_dir.join("intermediate.key.pem");
        let int_cert_path = ca_dir.join("intermediate.cert.pem");
        let root_cert_path = ca_dir.join("root.cert.pem");

        if int_key_path.exists() && int_cert_path.exists() {
            tracing::info!("loading existing intermediate CA");
            return Self::load(ca_dir);
        }

        tracing::info!("generating new CA chain");
        Self::generate(ca_dir, &int_key_path, &int_cert_path, &root_cert_path)
    }

    fn generate(
        ca_dir: &Path,
        int_key_path: &Path,
        int_cert_path: &Path,
        root_cert_path: &Path,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(ca_dir)?;

        // 1. Generate root CA
        let root_key = KeyPair::generate()?;
        let mut root_params = CertificateParams::new(Vec::<String>::new())?;
        root_params
            .distinguished_name
            .push(DnType::CommonName, "Seal Root CA");
        root_params
            .distinguished_name
            .push(DnType::OrganizationName, "Seal");
        root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        root_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        // Valid for 20 years
        root_params.not_before = time::OffsetDateTime::now_utc();
        root_params.not_after =
            time::OffsetDateTime::now_utc() + time::Duration::days(365 * 20);

        let root_cert = root_params.self_signed(&root_key)?;
        let root_cert_pem = root_cert.pem();

        // Save root cert (we keep this for trust store installation)
        std::fs::write(root_cert_path, &root_cert_pem)?;

        // 2. Generate intermediate CA constrained to *.seal
        let int_key = KeyPair::generate()?;
        let mut int_params = CertificateParams::new(Vec::<String>::new())?;
        int_params
            .distinguished_name
            .push(DnType::CommonName, "Seal Intermediate CA");
        int_params
            .distinguished_name
            .push(DnType::OrganizationName, "Seal");
        int_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        int_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        int_params.name_constraints = Some(NameConstraints {
            permitted_subtrees: vec![GeneralSubtree::DnsName(".seal".to_string())],
            excluded_subtrees: vec![],
        });
        int_params.not_before = time::OffsetDateTime::now_utc();
        int_params.not_after =
            time::OffsetDateTime::now_utc() + time::Duration::days(365 * 10);

        let int_cert = int_params.signed_by(&int_key, &root_cert, &root_key)?;

        // Save intermediate key + cert
        let int_key_pem = int_key.serialize_pem();
        let int_cert_pem = int_cert.pem();
        std::fs::write(int_key_path, &int_key_pem)?;
        std::fs::write(int_cert_path, &int_cert_pem)?;

        // 3. Delete root private key — never retained
        // (We only ever had it in memory; we just don't save it.)
        tracing::info!("root CA private key discarded (never written to disk)");
        tracing::info!("root CA certificate saved to {}", root_cert_path.display());
        tracing::info!(
            "add it to your system trust store: sudo cp {} /usr/local/share/ca-certificates/seal-root.crt && sudo update-ca-certificates",
            root_cert_path.display()
        );

        Self::from_pem(&int_key_pem, &int_cert_pem)
    }

    fn load(ca_dir: &Path) -> anyhow::Result<Self> {
        let int_key_pem = std::fs::read_to_string(ca_dir.join("intermediate.key.pem"))?;
        let int_cert_pem = std::fs::read_to_string(ca_dir.join("intermediate.cert.pem"))?;
        Self::from_pem(&int_key_pem, &int_cert_pem)
    }

    /// Returns true if the CA chain already exists on disk.
    pub fn exists(ca_dir: &Path) -> bool {
        ca_dir.join("intermediate.key.pem").exists() && ca_dir.join("intermediate.cert.pem").exists()
    }

    fn from_pem(key_pem: &str, cert_pem: &str) -> anyhow::Result<Self> {
        let intermediate_key = KeyPair::from_pem(key_pem)?;
        let int_cert_params = CertificateParams::from_ca_cert_pem(cert_pem)?;
        // self_signed() gives us a Certificate we can use as issuer in signed_by().
        // But its DER has a self-signature, not the root's signature.
        let intermediate_cert = int_cert_params.self_signed(&intermediate_key)?;

        // Parse the original PEM to get the DER with the root CA's real signature.
        // This is what we send in the cert chain to clients.
        let original_der = pem_to_der(cert_pem)?;

        Ok(Self {
            intermediate_key: Arc::new(intermediate_key),
            intermediate_cert: Arc::new(intermediate_cert),
            intermediate_cert_der: Arc::new(original_der),
            cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
        })
    }

    /// Issue (or return cached) leaf certificate for a .seal hostname.
    pub fn resolve(
        &self,
        hostname: &str,
    ) -> anyhow::Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
        // Check cache
        {
            let cache = self.cache.read().unwrap();
            if let Some(cached) = cache.get(hostname) {
                let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cached.key_der.clone()));
                return Ok((cached.cert_chain.clone(), key));
            }
        }

        // Issue new leaf cert
        let leaf_key = KeyPair::generate()?;
        let mut leaf_params = CertificateParams::new(Vec::<String>::new())?;
        leaf_params
            .distinguished_name
            .push(DnType::CommonName, hostname);
        leaf_params.subject_alt_names = vec![SanType::DnsName(hostname.try_into()?)];
        leaf_params.is_ca = IsCa::NoCa;
        leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        leaf_params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        leaf_params.not_before = time::OffsetDateTime::now_utc();
        leaf_params.not_after =
            time::OffsetDateTime::now_utc() + time::Duration::days(365);

        let leaf_cert =
            leaf_params.signed_by(&leaf_key, &self.intermediate_cert, &self.intermediate_key)?;

        let leaf_cert_der = CertificateDer::from(leaf_cert.der().to_vec());
        let leaf_key_raw = leaf_key.serialize_der();

        let cert_chain = vec![leaf_cert_der, (*self.intermediate_cert_der).clone()];

        // Cache it
        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(
                hostname.to_string(),
                CachedCert {
                    cert_chain: cert_chain.clone(),
                    key_der: leaf_key_raw.clone(),
                },
            );
        }

        let leaf_key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key_raw));
        Ok((cert_chain, leaf_key_der))
    }
}

/// Decode a PEM-encoded certificate to DER.
fn pem_to_der(pem: &str) -> anyhow::Result<CertificateDer<'static>> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    certs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no certificate found in PEM"))
}

/// Install the root CA certificate into the system trust store.
/// This typically requires root/sudo privileges.
pub fn install_trust_store(ca_dir: &Path) -> anyhow::Result<()> {
    let root_cert_path = ca_dir.join("root.cert.pem");
    if !root_cert_path.exists() {
        anyhow::bail!(
            "root certificate not found at {}. Run `seal init` first.",
            root_cert_path.display()
        );
    }

    if cfg!(target_os = "macos") {
        install_trust_store_macos(&root_cert_path)
    } else {
        install_trust_store_linux(&root_cert_path)
    }
}

/// Remove the root CA certificate from the system trust store.
pub fn uninstall_trust_store() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        uninstall_trust_store_macos()
    } else {
        uninstall_trust_store_linux()
    }
}

fn uninstall_trust_store_linux() -> anyhow::Result<()> {
    let dest = Path::new("/usr/local/share/ca-certificates/seal-root.crt");
    if dest.exists() {
        std::fs::remove_file(dest)?;
        eprintln!("removed {}", dest.display());

        let status = std::process::Command::new("update-ca-certificates")
            .arg("--fresh")
            .status()?;
        if !status.success() {
            anyhow::bail!("update-ca-certificates --fresh failed (exit {})", status);
        }
        eprintln!("root CA removed from system trust store");
    } else {
        eprintln!("root CA not in system trust store (already removed)");
    }
    Ok(())
}

fn uninstall_trust_store_macos() -> anyhow::Result<()> {
    eprintln!("removing root CA from macOS system keychain...");
    let status = std::process::Command::new("security")
        .args([
            "delete-certificate",
            "-c", "Seal Root CA",
            "-t",
        ])
        .status()?;

    if !status.success() {
        eprintln!("warning: could not remove cert from keychain (may already be removed)");
    } else {
        eprintln!("root CA removed from macOS system keychain");
    }
    Ok(())
}

fn install_trust_store_linux(root_cert_path: &Path) -> anyhow::Result<()> {
    let dest = std::path::Path::new("/usr/local/share/ca-certificates/seal-root.crt");
    eprintln!("copying {} -> {}", root_cert_path.display(), dest.display());
    std::fs::copy(root_cert_path, dest)?;

    eprintln!("running update-ca-certificates (the 'skipping ca-certificates.crt' warning is expected)...");
    let status = std::process::Command::new("update-ca-certificates")
        .status()?;

    if !status.success() {
        anyhow::bail!("update-ca-certificates failed (exit {})", status);
    }

    eprintln!("root CA installed in system trust store");
    Ok(())
}

fn install_trust_store_macos(root_cert_path: &Path) -> anyhow::Result<()> {
    eprintln!("adding root CA to macOS system keychain...");
    let status = std::process::Command::new("security")
        .args([
            "add-trusted-cert",
            "-d",
            "-r", "trustRoot",
            "-k", "/Library/Keychains/System.keychain",
        ])
        .arg(root_cert_path)
        .status()?;

    if !status.success() {
        anyhow::bail!("security add-trusted-cert failed (exit {})", status);
    }

    eprintln!("root CA installed in macOS system keychain");
    Ok(())
}
