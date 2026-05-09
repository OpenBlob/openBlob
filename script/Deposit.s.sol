// SPDX-License-Identifier: MIT
pragma solidity ^0.8.34;

import {Script, console2} from "forge-std/Script.sol";
import {OpenBlob} from "../src/OpenBlob.sol";

/// Actor A funds `OpenBlob` with collateral.
/// env: OPEN_BLOB_ADDR, ACTOR_A_PK, [BENEFICIARY], [DEPOSIT_AMOUNT_WEI]
contract Deposit is Script {
    function run() external {
        OpenBlob openBlob = OpenBlob(vm.envAddress("OPEN_BLOB_ADDR"));
        uint256 actorAPk = vm.envUint("ACTOR_A_PK");
        address actorA = vm.addr(actorAPk);
        address beneficiary = vm.envOr("BENEFICIARY", actorA);
        uint256 amount = vm.envOr("DEPOSIT_AMOUNT_WEI", uint256(0.1 ether));

        vm.startBroadcast(actorAPk);
        openBlob.deposit{value: amount}(beneficiary);
        vm.stopBroadcast();

        console2.log("Depositor:", actorA);
        console2.log("Beneficiary:", beneficiary);
        console2.log("Amount (wei):", amount);
        console2.log("Beneficiary balance now (wei):", openBlob.balances(beneficiary));
    }
}
