// SPDX-License-Identifier: MIT
pragma solidity ^0.8.34;

import {Script, console2} from "forge-std/Script.sol";

/// Actor B reads Actor A's signed transfer, dumps the blob payload bytes for
/// `cast send --blob`, and assembles the `proofBlobDA` calldata. With the
/// mock verifier in place, the proof bytes can be empty.
///
/// env: TRANSFER_FILE, [BLOB_FILE], [CALLDATA_FILE], PREV_ROOT, [NEW_ROOT],
///      [TOTAL_ETHER_PAID], PROVE_BLOCK_NUMBER, [PROOF_BYTES]
contract PrepareProof is Script {
    function run() external {
        string memory transferFile = vm.envOr("TRANSFER_FILE", string("./flow-artifacts/transfer.json"));
        string memory blobFile = vm.envOr("BLOB_FILE", string("./flow-artifacts/blob.bin"));
        string memory calldataFile = vm.envOr("CALLDATA_FILE", string("./flow-artifacts/proof.calldata"));

        string memory json = vm.readFile(transferFile);
        uint256 amount = vm.parseJsonUint(json, ".amount");
        bytes memory blobPayload = vm.parseJsonBytes(json, ".blobPayload");
        bytes32 hashedData = vm.parseJsonBytes32(json, ".hashedData");

        // Raw bytes for `cast send --blob --blob-file`. Cast handles field-element
        // encoding and zero-padding to 131072 bytes.
        vm.writeFileBinary(blobFile, blobPayload);

        uint256[] memory blobIndexes = new uint256[](1);
        blobIndexes[0] = 0;
        bytes32[] memory hashedDataArr = new bytes32[](1);
        hashedDataArr[0] = hashedData;

        bytes32 prevRoot = vm.envBytes32("PREV_ROOT");
        bytes32 newRoot = vm.envOr("NEW_ROOT", keccak256(abi.encode("post", hashedData)));
        uint256 totalEtherPaid = vm.envOr("TOTAL_ETHER_PAID", amount);
        uint256 blockNumber = vm.envUint("PROVE_BLOCK_NUMBER");
        bytes memory proof = vm.envOr("PROOF_BYTES", bytes(""));

        bytes memory cd = abi.encodeWithSignature(
            "proofBlobDA(uint256[],bytes32[],bytes32,bytes32,uint256,uint256,bytes)",
            blobIndexes,
            hashedDataArr,
            prevRoot,
            newRoot,
            totalEtherPaid,
            blockNumber,
            proof
        );

        // Write as 0x-prefixed hex so bash can `cat` it directly into `cast send`.
        vm.writeFile(calldataFile, vm.toString(cd));

        console2.log("Blob payload bytes:", blobPayload.length);
        console2.log("Blob file:", blobFile);
        console2.log("Calldata file:", calldataFile);
        console2.log("prevRoot:");
        console2.logBytes32(prevRoot);
        console2.log("newRoot:");
        console2.logBytes32(newRoot);
        console2.log("totalEtherPaid (wei):", totalEtherPaid);
        console2.log("blockNumber:", blockNumber);
    }
}
