// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

contract SealRegistry {
    struct AppRecord {
        string sealUrl;
        string name;
        uint256 keepAlive;       // timestamp of last keepAlive
        bytes32 versionKey;      // latest version
    }

    struct VersionRecord {
        address owner;
        string version;          // semver
        bytes32 bundleHash;      // keccak256 of the bundle
        uint256 bundleFormat;    // 1 = zip.br
        string[] bundleSources;  // base URLs serving <bundleHash>.zip.br
        uint256 publishedAt;     // block.timestamp
        string insecureMessage;  // empty = safe
        bytes32 previousVersionKey;
    }

    // (owner, sealId) => AppRecord
    mapping(address => mapping(bytes32 => AppRecord)) public apps;

    // versionKey => VersionRecord
    mapping(bytes32 => VersionRecord) internal _versions;

    // (owner, sealId) => allowed new owner
    mapping(address => mapping(bytes32 => address)) public allowedNewOwners;

    event Published(
        address indexed owner,
        bytes32 indexed sealId,
        bytes32 versionKey,
        string version,
        bytes32 bundleHash
    );
    event KeptAlive(address indexed owner, bytes32 indexed sealId, uint256 timestamp);
    event MarkedInsecure(bytes32 indexed versionKey, string message);
    event ClearedInsecure(bytes32 indexed versionKey);
    event NewOwnerAllowed(address indexed owner, bytes32 indexed sealId, address newOwner);

    /// Publish a new version (creates the app on first call).
    /// previousVersionKey must match the current versionKey (prevents races).
    function publish(
        string calldata sealUrl,
        string calldata name,
        string calldata version,
        bytes32 bundleHash,
        uint256 bundleFormat,
        string[] calldata bundleSources,
        bytes32 previousVersionKey
    ) external {
        bytes32 sealId = keccak256(bytes(sealUrl));
        AppRecord storage app = apps[msg.sender][sealId];

        require(app.versionKey == previousVersionKey, "version race: previousVersionKey mismatch");

        bytes32 versionKey = keccak256(abi.encode(msg.sender, bundleHash));
        require(_versions[versionKey].owner == address(0), "version already exists");

        // First publish — store sealUrl
        if (bytes(app.sealUrl).length == 0) {
            app.sealUrl = sealUrl;
        }
        app.name = name;
        app.keepAlive = block.timestamp;
        app.versionKey = versionKey;

        VersionRecord storage v = _versions[versionKey];
        v.owner = msg.sender;
        v.version = version;
        v.bundleHash = bundleHash;
        v.bundleFormat = bundleFormat;
        v.publishedAt = block.timestamp;
        v.previousVersionKey = previousVersionKey;

        for (uint256 i = 0; i < bundleSources.length; i++) {
            v.bundleSources.push(bundleSources[i]);
        }

        emit Published(msg.sender, sealId, versionKey, version, bundleHash);
    }

    /// Refresh keepAlive (annual heartbeat).
    function keepAlive(string calldata sealUrl) external {
        bytes32 sealId = keccak256(bytes(sealUrl));
        AppRecord storage app = apps[msg.sender][sealId];
        require(bytes(app.sealUrl).length > 0, "app not found");

        app.keepAlive = block.timestamp;
        emit KeptAlive(msg.sender, sealId, block.timestamp);
    }

    /// Mark a version as insecure (owner only).
    function markInsecure(bytes32 versionKey, string calldata message) external {
        require(_versions[versionKey].owner == msg.sender, "not version owner");
        require(bytes(message).length > 0, "message required");

        _versions[versionKey].insecureMessage = message;
        emit MarkedInsecure(versionKey, message);
    }

    /// Clear insecure flag (owner only).
    function clearInsecure(bytes32 versionKey) external {
        require(_versions[versionKey].owner == msg.sender, "not version owner");

        _versions[versionKey].insecureMessage = "";
        emit ClearedInsecure(versionKey);
    }

    /// Update bundle sources for a version (owner only).
    function updateBundleSources(bytes32 versionKey, string[] calldata bundleSources) external {
        require(_versions[versionKey].owner == msg.sender, "not version owner");

        delete _versions[versionKey].bundleSources;
        for (uint256 i = 0; i < bundleSources.length; i++) {
            _versions[versionKey].bundleSources.push(bundleSources[i]);
        }
    }

    /// Signal that a new owner is expected (prevents daemon warnings).
    function allowNewOwner(string calldata sealUrl, address newOwner) external {
        bytes32 sealId = keccak256(bytes(sealUrl));
        require(bytes(apps[msg.sender][sealId].sealUrl).length > 0, "app not found");

        allowedNewOwners[msg.sender][sealId] = newOwner;
        emit NewOwnerAllowed(msg.sender, sealId, newOwner);
    }

    // --- Views ---

    function getVersion(bytes32 versionKey) external view returns (
        address owner,
        string memory version,
        bytes32 bundleHash,
        uint256 bundleFormat,
        string[] memory bundleSources,
        uint256 publishedAt,
        string memory insecureMessage,
        bytes32 previousVersionKey
    ) {
        VersionRecord storage v = _versions[versionKey];
        require(v.owner != address(0), "version not found");
        return (
            v.owner,
            v.version,
            v.bundleHash,
            v.bundleFormat,
            v.bundleSources,
            v.publishedAt,
            v.insecureMessage,
            v.previousVersionKey
        );
    }

    function getApp(address owner, string calldata sealUrl) external view returns (
        string memory name,
        uint256 keepAliveTimestamp,
        bytes32 versionKey
    ) {
        bytes32 sealId = keccak256(bytes(sealUrl));
        AppRecord storage app = apps[owner][sealId];
        require(bytes(app.sealUrl).length > 0, "app not found");
        return (app.name, app.keepAlive, app.versionKey);
    }
}
