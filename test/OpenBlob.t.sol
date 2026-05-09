// SPDX-License-Identifier: MIT
pragma solidity ^0.8.34;

import {Test} from "forge-std/Test.sol";
import {OpenBlob} from "../src/OpenBlob.sol";
import {IVerifier} from "../src/IVerifier.sol";

contract MockVerifier is IVerifier {
    bool public ok = true;
    bytes32 public lastHash;

    function setOk(bool v) external {
        ok = v;
    }

    function setLastHash(bytes32 h) external {
        lastHash = h;
    }

    function verifyProof(bytes calldata, bytes32 publicInputsHash) external view returns (bool) {
        require(lastHash == bytes32(0) || lastHash == publicInputsHash, "hash mismatch");
        return ok;
    }
}

contract RevertingReceiver {
    OpenBlob public immutable target;

    constructor(OpenBlob t) {
        target = t;
    }

    function callProof(
        uint256[] calldata blobIndexes,
        bytes32[] calldata hashedData,
        bytes32 prevRoot,
        bytes32 newRoot,
        uint256 totalEtherPaid,
        uint256 blockNumber,
        bytes calldata proof
    ) external {
        target.proofBlobDA(blobIndexes, hashedData, prevRoot, newRoot, totalEtherPaid, blockNumber, proof);
    }

    receive() external payable {
        revert("nope");
    }
}

contract OpenBlobTest is Test {
    OpenBlob internal openBlob;
    MockVerifier internal verifier;

    address internal alice = address(0xA11CE);
    address internal bob = address(0xB0B);

    event Deposit(address indexed from, uint256 amount);
    event BlobDAProved(
        address indexed prover,
        bytes32 newRoot,
        uint256 totalExtracted,
        uint256 blockNumber
    );

    function setUp() public {
        verifier = new MockVerifier();
        openBlob = new OpenBlob(verifier);
    }

    // ------------------------------------------------------------------
    // deposit
    // ------------------------------------------------------------------

    function test_Deposit_CreditsBeneficiaryAndEmits() public {
        vm.deal(alice, 5 ether);

        vm.expectEmit(true, false, false, true, address(openBlob));
        emit Deposit(bob, 2 ether);

        vm.prank(alice);
        openBlob.deposit{value: 2 ether}(bob);

        assertEq(openBlob.balances(bob), 2 ether, "bob credited");
        assertEq(openBlob.balances(alice), 0, "alice not credited");
        assertEq(address(openBlob).balance, 2 ether, "contract holds the eth");
    }

    function test_Deposit_AccumulatesAcrossCalls() public {
        vm.deal(alice, 10 ether);
        vm.startPrank(alice);
        openBlob.deposit{value: 1 ether}(alice);
        openBlob.deposit{value: 3 ether}(alice);
        vm.stopPrank();
        assertEq(openBlob.balances(alice), 4 ether);
    }

    // ------------------------------------------------------------------
    // proofBlobDA — happy path
    // ------------------------------------------------------------------

    function test_ProofBlobDA_HappyPath() public {
        // Fund the contract so it can pay the prover.
        vm.deal(alice, 10 ether);
        vm.prank(alice);
        openBlob.deposit{value: 5 ether}(alice);

        // Inject blobhashes for indexes 0 and 1.
        bytes32[] memory injected = new bytes32[](2);
        injected[0] = keccak256("blob-0");
        injected[1] = keccak256("blob-1");
        vm.blobhashes(injected);

        // Build the proofBlobDA inputs.
        uint256[] memory blobIndexes = new uint256[](2);
        blobIndexes[0] = 0;
        blobIndexes[1] = 1;

        bytes32[] memory hashedData = new bytes32[](2);
        hashedData[0] = keccak256("payload-0");
        hashedData[1] = keccak256("payload-1");

        bytes32 prevRoot = openBlob.openBlobRoot();
        bytes32 newRoot = keccak256("new-root");
        uint256 totalEtherPaid = 1 ether;
        uint256 blockNumber = block.number;

        // Mirror the on-chain digest so the mock verifier asserts we got the right bytes.
        bytes32 expectedHash = keccak256(
            abi.encode(
                injected,
                hashedData,
                prevRoot,
                newRoot,
                totalEtherPaid,
                blockhash(blockNumber)
            )
        );
        verifier.setLastHash(expectedHash);

        uint256 bobBefore = bob.balance;

        vm.expectEmit(true, false, false, true, address(openBlob));
        emit BlobDAProved(bob, newRoot, totalEtherPaid, blockNumber);

        vm.prank(bob);
        openBlob.proofBlobDA(
            blobIndexes,
            hashedData,
            prevRoot,
            newRoot,
            totalEtherPaid,
            blockNumber,
            hex"deadbeef"
        );

        assertEq(openBlob.openBlobRoot(), newRoot, "root rotated");
        assertTrue(openBlob.dataAvailable(hashedData[0]), "data 0 marked available");
        assertTrue(openBlob.dataAvailable(hashedData[1]), "data 1 marked available");
        assertEq(bob.balance - bobBefore, totalEtherPaid, "prover paid");
        assertEq(address(openBlob).balance, 5 ether - totalEtherPaid, "contract debited");
    }

    // ------------------------------------------------------------------
    // proofBlobDA — revert paths
    // ------------------------------------------------------------------

    function test_RevertWhen_LengthMismatch() public {
        uint256[] memory blobIndexes = new uint256[](2);
        bytes32[] memory hashedData = new bytes32[](1);

        vm.expectRevert(OpenBlob.LengthMismatch.selector);
        openBlob.proofBlobDA(blobIndexes, hashedData, bytes32(0), bytes32(0), 0, block.number, "");
    }

    function test_RevertWhen_PrevRootMismatch() public {
        uint256[] memory blobIndexes = new uint256[](0);
        bytes32[] memory hashedData = new bytes32[](0);
        bytes32 wrongPrev = bytes32(uint256(0xdead));

        vm.expectRevert(
            abi.encodeWithSelector(OpenBlob.PrevRootMismatch.selector, bytes32(0), wrongPrev)
        );
        openBlob.proofBlobDA(blobIndexes, hashedData, wrongPrev, bytes32(0), 0, block.number, "");
    }

    function test_RevertWhen_BlobIndexNotIncreasing() public {
        bytes32[] memory injected = new bytes32[](3);
        injected[0] = keccak256("a");
        injected[1] = keccak256("b");
        injected[2] = keccak256("c");
        vm.blobhashes(injected);

        uint256[] memory blobIndexes = new uint256[](2);
        blobIndexes[0] = 1;
        blobIndexes[1] = 1; // not strictly increasing

        bytes32[] memory hashedData = new bytes32[](2);
        hashedData[0] = keccak256("h0");
        hashedData[1] = keccak256("h1");

        vm.expectRevert(
            abi.encodeWithSelector(OpenBlob.BlobIndexNotIncreasing.selector, uint256(1), uint256(1))
        );
        openBlob.proofBlobDA(blobIndexes, hashedData, bytes32(0), bytes32(0), 0, block.number, "");
    }

    function test_RevertWhen_BlobNotFound() public {
        // No blobhashes injected; blobhash(0) returns zero.
        uint256[] memory blobIndexes = new uint256[](1);
        blobIndexes[0] = 0;

        bytes32[] memory hashedData = new bytes32[](1);
        hashedData[0] = keccak256("h");

        vm.expectRevert(abi.encodeWithSelector(OpenBlob.BlobNotFound.selector, uint256(0)));
        openBlob.proofBlobDA(blobIndexes, hashedData, bytes32(0), bytes32(0), 0, block.number, "");
    }

    function test_RevertWhen_InvalidProof() public {
        bytes32[] memory injected = new bytes32[](1);
        injected[0] = keccak256("blob");
        vm.blobhashes(injected);

        uint256[] memory blobIndexes = new uint256[](1);
        blobIndexes[0] = 0;
        bytes32[] memory hashedData = new bytes32[](1);
        hashedData[0] = keccak256("h");

        verifier.setOk(false);

        vm.expectRevert(OpenBlob.InvalidProof.selector);
        openBlob.proofBlobDA(blobIndexes, hashedData, bytes32(0), bytes32(0), 0, block.number, "");
    }

    function test_RevertWhen_TransferFails() public {
        RevertingReceiver bad = new RevertingReceiver(openBlob);
        // Fund the contract.
        vm.deal(address(this), 10 ether);
        openBlob.deposit{value: 5 ether}(address(this));

        bytes32[] memory injected = new bytes32[](1);
        injected[0] = keccak256("blob");
        vm.blobhashes(injected);

        uint256[] memory blobIndexes = new uint256[](1);
        blobIndexes[0] = 0;
        bytes32[] memory hashedData = new bytes32[](1);
        hashedData[0] = keccak256("h");

        bytes32 prevRoot = openBlob.openBlobRoot();
        vm.expectRevert(OpenBlob.TransferFailed.selector);
        bad.callProof(blobIndexes, hashedData, prevRoot, keccak256("new"), 1 ether, block.number, "");
    }

    // ------------------------------------------------------------------
    // proofBlobDA — state effects after success
    // ------------------------------------------------------------------

    function test_ProofBlobDA_RootRotationBlocksReplay() public {
        // First successful proof rotates the root; submitting the same
        // prevRoot a second time must now fail.
        vm.deal(alice, 10 ether);
        vm.prank(alice);
        openBlob.deposit{value: 5 ether}(alice);

        bytes32[] memory injected = new bytes32[](1);
        injected[0] = keccak256("blob");
        vm.blobhashes(injected);

        uint256[] memory blobIndexes = new uint256[](1);
        blobIndexes[0] = 0;
        bytes32[] memory hashedData = new bytes32[](1);
        hashedData[0] = keccak256("h");

        bytes32 prevRoot = openBlob.openBlobRoot();
        bytes32 newRoot = keccak256("rot");

        vm.prank(bob);
        openBlob.proofBlobDA(blobIndexes, hashedData, prevRoot, newRoot, 1 ether, block.number, "");

        assertEq(openBlob.openBlobRoot(), newRoot);

        // Replay with the now-stale prevRoot must revert.
        vm.expectRevert(
            abi.encodeWithSelector(OpenBlob.PrevRootMismatch.selector, newRoot, prevRoot)
        );
        vm.prank(bob);
        openBlob.proofBlobDA(blobIndexes, hashedData, prevRoot, newRoot, 1 ether, block.number, "");
    }

    // ------------------------------------------------------------------
    // gas snapshots
    // ------------------------------------------------------------------

    function _recordSnapshot() internal {
        string[] memory cmd = new string[](2);
        cmd[0] = "bash";
        cmd[1] = "script/.snapshot.sh";
        emit log_named_string("snapshot", string(vm.ffi(cmd)));
    }

    function test_GasSnapshot_DepositBaseline() public {
        _recordSnapshot();

        vm.deal(alice, 1 ether);
        vm.prank(alice);
        openBlob.deposit{value: 1 ether}(alice);
        assertEq(openBlob.balances(alice), 1 ether);
    }

    receive() external payable {}
}
