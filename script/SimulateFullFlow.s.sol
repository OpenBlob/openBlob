// SPDX-License-Identifier: MIT
pragma solidity ^0.8.34;

import {Script, console2} from "forge-std/Script.sol";
import {OpenBlob} from "../src/OpenBlob.sol";
import {IVerifier} from "../src/IVerifier.sol";

contract SimMockVerifier is IVerifier {
    function verifyProof(bytes calldata, bytes32) external pure returns (bool) {
        return true;
    }
}

/// All-in-one local simulation: deploy mock + OpenBlob, A deposits, A signs,
/// B builds a payload, B injects a blobhash via `vm.blobhashes` and proves.
/// Run on anvil or in-memory — no real type-3 tx needed.
///
/// env: [ACTOR_A_PK], [ACTOR_B_PK], [AMOUNT_WEI]
contract SimulateFullFlow is Script {
    function run() external {
        uint256 actorAPk = vm.envOr("ACTOR_A_PK", uint256(0xA11CE));
        uint256 actorBPk = vm.envOr("ACTOR_B_PK", uint256(0xB0B));
        address actorA = vm.addr(actorAPk);
        address actorB = vm.addr(actorBPk);
        uint256 amount = vm.envOr("AMOUNT_WEI", uint256(0.1 ether));

        vm.deal(actorA, 1 ether);
        vm.deal(actorB, 1 ether);

        SimMockVerifier verifier = new SimMockVerifier();
        OpenBlob openBlob = new OpenBlob(verifier);
        console2.log("OpenBlob:", address(openBlob));

        vm.prank(actorA);
        openBlob.deposit{value: amount}(actorA);

        bytes memory message = abi.encode(
            address(openBlob), block.chainid, actorA, actorB, amount, uint256(0)
        );
        bytes32 ethSignedHash = keccak256(
            abi.encodePacked("\x19Ethereum Signed Message:\n32", keccak256(message))
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(actorAPk, ethSignedHash);
        bytes memory signature = abi.encodePacked(r, s, v);

        bytes memory blobPayload = abi.encode(actorA, actorB, amount, uint256(0), signature);
        bytes32 hashedData = keccak256(blobPayload);

        // Stand in for the versioned hash a real type-3 tx would contribute.
        bytes32[] memory blobhashes = new bytes32[](1);
        blobhashes[0] = bytes32(uint256(0x01) << 248) | (hashedData >> 8);
        vm.blobhashes(blobhashes);

        uint256[] memory blobIndexes = new uint256[](1);
        blobIndexes[0] = 0;
        bytes32[] memory hashedDataArr = new bytes32[](1);
        hashedDataArr[0] = hashedData;

        bytes32 prevRoot = openBlob.openBlobRoot();
        bytes32 newRoot = keccak256(abi.encode("post", hashedData));
        uint256 bBefore = actorB.balance;

        vm.prank(actorB);
        openBlob.proofBlobDA(blobIndexes, hashedDataArr, prevRoot, newRoot, amount, block.number, "");

        console2.log("B claimed (wei):", actorB.balance - bBefore);
        console2.log("dataAvailable:", openBlob.dataAvailable(hashedData));
        console2.log("openBlobRoot:");
        console2.logBytes32(openBlob.openBlobRoot());
    }
}
