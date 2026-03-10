// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "forge-std/Test.sol";
import "../src/SealRegistry.sol";

contract SealRegistryTest is Test {
    SealRegistry registry;
    address alice = makeAddr("alice");
    address bob = makeAddr("bob");

    string constant SEAL_URL = "https://example--com.seal/app";
    string constant APP_NAME = "Example App";

    function setUp() public {
        registry = new SealRegistry();
    }

    // --- publish ---

    function test_publish_first_version() public {
        bytes32 bundleHash = keccak256("bundle-v1");
        string[] memory sources = new string[](1);
        sources[0] = "https://cdn.example.com/bundles/";

        vm.prank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", bundleHash, 1, sources, bytes32(0));

        (string memory name, uint256 keepAliveTs, bytes32 versionKey) =
            registry.getApp(alice, SEAL_URL);

        assertEq(name, APP_NAME);
        assertGt(keepAliveTs, 0);

        bytes32 expectedKey = keccak256(abi.encode(alice, bundleHash));
        assertEq(versionKey, expectedKey);

        (
            address owner,
            string memory version,
            bytes32 hash,
            uint256 format,
            string[] memory srcs,
            uint256 publishedAt,
            string memory insecureMsg,
            bytes32 prevKey
        ) = registry.getVersion(versionKey);

        assertEq(owner, alice);
        assertEq(version, "1.0.0");
        assertEq(hash, bundleHash);
        assertEq(format, 1);
        assertEq(srcs.length, 1);
        assertEq(srcs[0], sources[0]);
        assertGt(publishedAt, 0);
        assertEq(bytes(insecureMsg).length, 0);
        assertEq(prevKey, bytes32(0));
    }

    function test_publish_second_version() public {
        bytes32 hash1 = keccak256("bundle-v1");
        bytes32 hash2 = keccak256("bundle-v2");
        string[] memory sources = new string[](0);

        vm.startPrank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", hash1, 1, sources, bytes32(0));

        (, , bytes32 key1) = registry.getApp(alice, SEAL_URL);
        registry.publish(SEAL_URL, APP_NAME, "2.0.0", hash2, 1, sources, key1);
        vm.stopPrank();

        (, , bytes32 key2) = registry.getApp(alice, SEAL_URL);
        assertNotEq(key1, key2);

        (, , , , , , , bytes32 prevKey) = registry.getVersion(key2);
        assertEq(prevKey, key1);
    }

    function test_publish_revert_on_version_race() public {
        bytes32 hash1 = keccak256("bundle-v1");
        bytes32 hash2 = keccak256("bundle-v2");
        string[] memory sources = new string[](0);

        vm.startPrank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", hash1, 1, sources, bytes32(0));

        // Try to publish with wrong previousVersionKey
        vm.expectRevert("version race: previousVersionKey mismatch");
        registry.publish(SEAL_URL, APP_NAME, "2.0.0", hash2, 1, sources, bytes32(0));
        vm.stopPrank();
    }

    function test_publish_revert_duplicate_version() public {
        bytes32 bundleHash = keccak256("bundle-v1");
        string[] memory sources = new string[](0);

        vm.prank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", bundleHash, 1, sources, bytes32(0));

        // Bob tries same bundleHash — same versionKey since keccak256(abi.encode(bob, bundleHash)) differs,
        // but alice's version occupies keccak256(abi.encode(alice, bundleHash)).
        // Actually bob's versionKey is different, so let's test alice re-publishing same hash.
        (, , bytes32 key1) = registry.getApp(alice, SEAL_URL);

        vm.prank(alice);
        vm.expectRevert("version already exists");
        registry.publish(SEAL_URL, APP_NAME, "1.0.1", bundleHash, 1, sources, key1);
    }

    // --- Multiple owners ---

    function test_multiple_owners_same_sealUrl() public {
        bytes32 hashA = keccak256("alice-bundle");
        bytes32 hashB = keccak256("bob-bundle");
        string[] memory sources = new string[](0);

        vm.prank(alice);
        registry.publish(SEAL_URL, "Alice's App", "1.0.0", hashA, 1, sources, bytes32(0));

        vm.prank(bob);
        registry.publish(SEAL_URL, "Bob's App", "1.0.0", hashB, 1, sources, bytes32(0));

        (string memory nameA, , ) = registry.getApp(alice, SEAL_URL);
        (string memory nameB, , ) = registry.getApp(bob, SEAL_URL);
        assertEq(nameA, "Alice's App");
        assertEq(nameB, "Bob's App");
    }

    // --- keepAlive ---

    function test_keepAlive() public {
        bytes32 bundleHash = keccak256("bundle");
        string[] memory sources = new string[](0);

        vm.prank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", bundleHash, 1, sources, bytes32(0));

        vm.warp(block.timestamp + 365 days);

        vm.prank(alice);
        registry.keepAlive(SEAL_URL);

        (, uint256 keepAliveTs, ) = registry.getApp(alice, SEAL_URL);
        assertEq(keepAliveTs, block.timestamp);
    }

    function test_keepAlive_revert_not_found() public {
        vm.prank(alice);
        vm.expectRevert("app not found");
        registry.keepAlive(SEAL_URL);
    }

    // --- markInsecure / clearInsecure ---

    function test_markInsecure_and_clear() public {
        bytes32 bundleHash = keccak256("bundle");
        string[] memory sources = new string[](0);

        vm.prank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", bundleHash, 1, sources, bytes32(0));

        (, , bytes32 versionKey) = registry.getApp(alice, SEAL_URL);

        vm.prank(alice);
        registry.markInsecure(versionKey, "XSS vulnerability");

        (, , , , , , string memory msg, ) = registry.getVersion(versionKey);
        assertEq(msg, "XSS vulnerability");

        vm.prank(alice);
        registry.clearInsecure(versionKey);

        (, , , , , , string memory cleared, ) = registry.getVersion(versionKey);
        assertEq(bytes(cleared).length, 0);
    }

    function test_markInsecure_revert_not_owner() public {
        bytes32 bundleHash = keccak256("bundle");
        string[] memory sources = new string[](0);

        vm.prank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", bundleHash, 1, sources, bytes32(0));

        (, , bytes32 versionKey) = registry.getApp(alice, SEAL_URL);

        vm.prank(bob);
        vm.expectRevert("not version owner");
        registry.markInsecure(versionKey, "hacked");
    }

    function test_markInsecure_revert_empty_message() public {
        bytes32 bundleHash = keccak256("bundle");
        string[] memory sources = new string[](0);

        vm.prank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", bundleHash, 1, sources, bytes32(0));

        (, , bytes32 versionKey) = registry.getApp(alice, SEAL_URL);

        vm.prank(alice);
        vm.expectRevert("message required");
        registry.markInsecure(versionKey, "");
    }

    // --- updateBundleSources ---

    function test_updateBundleSources() public {
        bytes32 bundleHash = keccak256("bundle");
        string[] memory sources = new string[](1);
        sources[0] = "https://old.example.com/";

        vm.prank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", bundleHash, 1, sources, bytes32(0));

        (, , bytes32 versionKey) = registry.getApp(alice, SEAL_URL);

        string[] memory newSources = new string[](2);
        newSources[0] = "https://cdn1.example.com/";
        newSources[1] = "https://cdn2.example.com/";

        vm.prank(alice);
        registry.updateBundleSources(versionKey, newSources);

        (, , , , string[] memory srcs, , , ) = registry.getVersion(versionKey);
        assertEq(srcs.length, 2);
        assertEq(srcs[0], newSources[0]);
        assertEq(srcs[1], newSources[1]);
    }

    function test_updateBundleSources_revert_not_owner() public {
        bytes32 bundleHash = keccak256("bundle");
        string[] memory sources = new string[](0);

        vm.prank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", bundleHash, 1, sources, bytes32(0));

        (, , bytes32 versionKey) = registry.getApp(alice, SEAL_URL);

        vm.prank(bob);
        vm.expectRevert("not version owner");
        registry.updateBundleSources(versionKey, sources);
    }

    // --- allowNewOwner ---

    function test_allowNewOwner() public {
        bytes32 bundleHash = keccak256("bundle");
        string[] memory sources = new string[](0);

        vm.prank(alice);
        registry.publish(SEAL_URL, APP_NAME, "1.0.0", bundleHash, 1, sources, bytes32(0));

        vm.prank(alice);
        registry.allowNewOwner(SEAL_URL, bob);

        bytes32 sealId = keccak256(bytes(SEAL_URL));
        assertEq(registry.allowedNewOwners(alice, sealId), bob);
    }

    function test_allowNewOwner_revert_not_found() public {
        vm.prank(alice);
        vm.expectRevert("app not found");
        registry.allowNewOwner(SEAL_URL, bob);
    }

    // --- getVersion not found ---

    function test_getVersion_revert_not_found() public {
        vm.expectRevert("version not found");
        registry.getVersion(bytes32(uint256(999)));
    }

    // --- getApp not found ---

    function test_getApp_revert_not_found() public {
        vm.expectRevert("app not found");
        registry.getApp(alice, SEAL_URL);
    }
}
