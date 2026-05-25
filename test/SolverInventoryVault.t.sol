// SPDX-License-Identifier: MIT
pragma solidity ^0.8.25;

import "forge-std/Test.sol";
import "../contracts/SolverInventoryVault.sol";
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";

/// Simple ERC20 for testing.
contract TestUSDC is ERC20 {
    constructor() ERC20("TestUSDC", "tUSDC") {
        _mint(msg.sender, 1_000_000e6);
    }

    function decimals() public view virtual override returns (uint8) {
        return 6;
    }
}

contract SolverInventoryVaultTest is Test {
    SolverInventoryVault vault;
    TestUSDC usdc;

    address lp = address(0x1);
    address solver = address(0x2);
    address owner;

    function setUp() public {
        owner = address(this);
        usdc = new TestUSDC();
        vault = new SolverInventoryVault(
            IERC20(address(usdc)),
            "SolverShare USDC",
            "sUSDC",
            500_000e6,  // lpCap
            50_000e6,   // maxPerBlock
            5           // killThreshold
        );
    }

    function testDeposit() public {
        usdc.transfer(lp, 100_000e6);
        vm.startPrank(lp);
        usdc.approve(address(vault), 100_000e6);
        uint256 shares = vault.deposit(100_000e6, lp);
        assertEq(shares, 100_000e6, "1:1 share price at inception");
        assertEq(vault.balanceOf(lp), 100_000e6);
        vm.stopPrank();
    }

    function testDepositExceedsLpCap() public {
        usdc.transfer(lp, 1_000_000e6);
        vm.startPrank(lp);
        usdc.approve(address(vault), 1_000_000e6);
        vm.expectRevert("vault: exceeds LP cap");
        vault.deposit(600_000e6, lp);
        vm.stopPrank();
    }

    function testDistributeRebate() public {
        // Deposit first
        usdc.transfer(lp, 100_000e6);
        vm.startPrank(lp);
        usdc.approve(address(vault), 100_000e6);
        vault.deposit(100_000e6, lp);
        vm.stopPrank();

        // Distribute rebate
        vault.distributeRebate(1_000e6);

        // Total assets should reflect the rebate
        assertEq(vault.totalAssets(), 101_000e6);
    }

    function testConsumeInventory() public {
        // Deposit first
        usdc.transfer(lp, 100_000e6);
        vm.startPrank(lp);
        usdc.approve(address(vault), 100_000e6);
        vault.deposit(100_000e6, lp);
        vm.stopPrank();

        // Consume inventory
        vault.consumeInventory(10_000e6, 1);
        assertEq(vault.totalAssets(), 90_000e6);
    }

    function testConsumeExceedsPerBlock() public {
        usdc.transfer(lp, 100_000e6);
        vm.startPrank(lp);
        usdc.approve(address(vault), 100_000e6);
        vault.deposit(100_000e6, lp);
        vm.stopPrank();

        vault.consumeInventory(40_000e6, 1);
        vm.expectRevert("vault: exceeds per-block inventory cap");
        vault.consumeInventory(20_000e6, 2);
    }

    function testKillSwitchOnAdverseStreak() public {
        for (uint256 i = 0; i < 5; i++) {
            vault.reportFillOutcome(false);
        }
        assertTrue(vault.killed());
    }

    function testKillSwitchRecover() public {
        for (uint256 i = 0; i < 5; i++) {
            vault.reportFillOutcome(false);
        }
        assertTrue(vault.killed());

        vault.recover();
        assertFalse(vault.killed());
        assertEq(vault.adverseStreak(), 0);
    }

    function testRebateStopsWhenKilled() public {
        for (uint256 i = 0; i < 5; i++) {
            vault.reportFillOutcome(false);
        }
        vm.expectRevert("vault: killed");
        vault.distributeRebate(1_000e6);
    }

    function testWithdraw() public {
        usdc.transfer(lp, 100_000e6);
        vm.startPrank(lp);
        usdc.approve(address(vault), 100_000e6);
        uint256 shares = vault.deposit(100_000e6, lp);

        uint256 assets = vault.redeem(shares, lp, lp);
        assertEq(assets, 100_000e6);
        vm.stopPrank();
    }
}
