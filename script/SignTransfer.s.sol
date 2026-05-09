// SPDX-License-Identifier: MIT
pragma solidity ^0.8.34;

import {Script, console2} from "forge-std/Script.sol";

/// Actor A signs a transfer authorization. The on-chain contract never reads
/// this signature — the ZK circuit (real or mock) is what would verify it
/// after decoding the blob payload. We persist (message, sig, payload,
/// hashedData) so the prover can pick them up.
///
/// env: OPEN_BLOB_ADDR, ACTOR_A_PK, RECIPIENT, [AMOUNT_WEI], [NONCE], [OUT_FILE]
contract SignTransfer is Script {
    function run() external {
        address openBlobAddr = vm.envAddress("OPEN_BLOB_ADDR");
        uint256 actorAPk = vm.envUint("ACTOR_A_PK");
        address actorA = vm.addr(actorAPk);
        address recipient = vm.envAddress("RECIPIENT");
        uint256 amount = vm.envOr("AMOUNT_WEI", uint256(0.1 ether));
        uint256 nonce = vm.envOr("NONCE", uint256(0));
        string memory outFile = vm.envOr("OUT_FILE", string("./flow-artifacts/transfer.json"));

        // Application message. The circuit decides this layout; we mirror it
        // so the on-chain `hashedData` lines up with what the circuit derives.
        bytes memory message = abi.encode(
            openBlobAddr, block.chainid, actorA, recipient, amount, nonce
        );
        bytes32 messageHash = keccak256(message);
        bytes32 ethSignedHash = keccak256(
            abi.encodePacked("\x19Ethereum Signed Message:\n32", messageHash)
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(actorAPk, ethSignedHash);
        bytes memory signature = abi.encodePacked(r, s, v);

        // Blob payload = the message fields + signature. The prover stuffs
        // this into a blob; the circuit decodes and recovers `actorA`.
        bytes memory blobPayload = abi.encode(actorA, recipient, amount, nonce, signature);
        bytes32 hashedData = keccak256(blobPayload);

        string memory key = "transfer";
        vm.serializeAddress(key, "openBlob", openBlobAddr);
        vm.serializeUint(key, "chainId", block.chainid);
        vm.serializeAddress(key, "actorA", actorA);
        vm.serializeAddress(key, "recipient", recipient);
        vm.serializeUint(key, "amount", amount);
        vm.serializeUint(key, "nonce", nonce);
        vm.serializeBytes32(key, "messageHash", messageHash);
        vm.serializeBytes(key, "signature", signature);
        vm.serializeBytes(key, "blobPayload", blobPayload);
        string memory finalJson = vm.serializeBytes32(key, "hashedData", hashedData);
        vm.writeJson(finalJson, outFile);

        console2.log("Wrote:", outFile);
        console2.log("Signer:", actorA);
        console2.log("Recipient:", recipient);
        console2.log("Amount (wei):", amount);
        console2.log("hashedData:");
        console2.logBytes32(hashedData);
    }
}
