// SPDX-License-Identifier: MIT
pragma solidity ^0.8.34;

import {Script, console2} from "forge-std/Script.sol";
import {OpenBlob} from "../src/OpenBlob.sol";
import {IVerifier} from "../src/IVerifier.sol";

/// Always-true verifier. Smoke-test only — never deploy this on a real network.
contract MockVerifier is IVerifier {
    function verifyProof(bytes calldata, bytes32) external pure returns (bool) {
        return true;
    }
}

/// Deploy `OpenBlob` against a real, already-deployed verifier.
/// env: VERIFIER_ADDR
contract DeployOpenBlob is Script {
    function run() external returns (OpenBlob openBlob) {
        address verifierAddr = vm.envAddress("VERIFIER_ADDR");
        vm.startBroadcast();
        openBlob = new OpenBlob(IVerifier(verifierAddr));
        vm.stopBroadcast();
        console2.log("Verifier:", verifierAddr);
        console2.log("OpenBlob:", address(openBlob));
    }
}

/// Deploy a fresh `MockVerifier` plus an `OpenBlob` wired to it.
contract DeployOpenBlobMock is Script {
    function run() external returns (OpenBlob openBlob, MockVerifier verifier) {
        vm.startBroadcast();
        verifier = new MockVerifier();
        openBlob = new OpenBlob(verifier);
        vm.stopBroadcast();
        console2.log("MockVerifier:", address(verifier));
        console2.log("OpenBlob:", address(openBlob));
    }
}
