// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {Script, console} from "forge-std/Script.sol";
import {SealRegistry} from "../src/SealRegistry.sol";

contract DeployScript is Script {
    // Nick's deterministic deployer factory, available on all major EVM chains.
    address constant NICK_FACTORY = 0x4e59b44847b379578588920cA78FbF26c0B4956C;

    bytes32 constant SALT = bytes32(0);

    function run() external {
        bytes memory creationCode = type(SealRegistry).creationCode;
        address predicted = computeAddress(SALT, keccak256(creationCode), NICK_FACTORY);

        if (predicted.code.length > 0) {
            console.log("SealRegistry already deployed at:", predicted);
            return;
        }

        console.log("Deploying SealRegistry to:", predicted);

        vm.broadcast();
        (bool success,) = NICK_FACTORY.call(abi.encodePacked(SALT, creationCode));
        require(success, "CREATE2 deploy failed");

        require(predicted.code.length > 0, "deployment verification failed");
        console.log("Deployed successfully");
    }

    function computeAddress(bytes32 salt, bytes32 codeHash, address deployer) internal pure returns (address) {
        return address(uint160(uint256(keccak256(abi.encodePacked(bytes1(0xff), deployer, salt, codeHash)))));
    }
}
