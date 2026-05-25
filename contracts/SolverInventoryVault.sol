// SPDX-License-Identifier: MIT
pragma solidity ^0.8.25;

import {ERC4626} from "@openzeppelin/contracts/token/ERC20/extensions/ERC4626.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";

/// @title SolverInventoryVault
/// @notice ERC-4626 vault for solver inventory funding. LPs deposit USDC,
///         share in per-fill rebates from resolver spreads.
/// @dev Each chain deploys its own vault.
contract SolverInventoryVault is ERC4626, Ownable {
    using SafeERC20 for IERC20;

    // ── Events ──────────────────────────────────────────────

    event RebateDistributed(uint256 totalRebate, uint256 sharesValue, uint256 timestamp);
    event RiskParamsUpdated(uint256 lpCap, uint256 maxPerBlock, uint256 killThreshold);
    event InventoryConsumed(address solver, uint256 amount, uint256 fillId);

    // ── State ───────────────────────────────────────────────

    /// Maximum USDC per LP (18 decimals).
    uint256 public lpCap;

    /// Maximum inventory consumed per block.
    uint256 public maxPerBlock;

    /// Adverse fill streak threshold for kill-switch.
    uint256 public killThreshold;

    /// Current adverse fill streak.
    uint256 public adverseStreak;

    /// Inventory consumed in current block.
    uint256 public blockConsumed;

    /// Last block when consumption was reset.
    uint256 public lastBlock;

    /// Whether the vault is killed (emergency stop).
    bool public killed;

    // ── Constructor ──────────────────────────────────────────

    constructor(
        IERC20 asset_,
        string memory name_,
        string memory symbol_,
        uint256 lpCap_,
        uint256 maxPerBlock_,
        uint256 killThreshold_
    ) ERC4626(asset_) Ownable(msg.sender) {
        lpCap = lpCap_;
        maxPerBlock = maxPerBlock_;
        killThreshold = killThreshold_;
        lastBlock = block.number;
    }

    // ── Overrides ────────────────────────────────────────────

    /// @dev Enforce per-LP cap on mint.
    function _beforeDeposit(address caller, uint256 assets, uint256 shares) internal view override {
        if (killed) revert("vault: killed");
        if (totalAssets() + assets > lpCap) revert("vault: exceeds LP cap");
    }

    // ── Solver hooks ─────────────────────────────────────────

    /// @notice Called by the resolver after each profitable fill.
    ///         Distributes `rebateBps` of the captured spread to vault share holders.
    /// @param totalRebate Total rebate in asset units.
    function distributeRebate(uint256 totalRebate) external onlyOwner {
        if (killed) revert("vault: killed");

        // Reset block consumption counter if new block
        if (block.number != lastBlock) {
            blockConsumed = 0;
            lastBlock = block.number;
        }

        uint256 sharesValue = convertToShares(totalRebate);
        _mint(address(this), sharesValue);

        emit RebateDistributed(totalRebate, sharesValue, block.timestamp);
    }

    /// @notice Consume inventory for a fill.
    /// @param amount Amount of assets to consume.
    /// @param fillId Unique fill identifier.
    function consumeInventory(uint256 amount, uint256 fillId) external onlyOwner {
        if (killed) revert("vault: killed");

        if (block.number != lastBlock) {
            blockConsumed = 0;
            lastBlock = block.number;
        }

        if (blockConsumed + amount > maxPerBlock) {
            revert("vault: exceeds per-block inventory cap");
        }

        blockConsumed += amount;
        IERC20(asset()).safeTransfer(msg.sender, amount);

        emit InventoryConsumed(msg.sender, amount, fillId);
    }

    // ── Risk management ─────────────────────────────────────

    /// @notice Report a fill outcome (success or failure) to track adverse streaks.
    /// @param success Whether the fill was profitable.
    function reportFillOutcome(bool success) external onlyOwner {
        if (success) {
            adverseStreak = 0;
        } else {
            adverseStreak++;
            if (adverseStreak >= killThreshold) {
                killed = true;
                emit RiskParamsUpdated(lpCap, maxPerBlock, killThreshold);
            }
        }
    }

    /// @notice Update risk parameters.
    function updateRiskParams(uint256 lpCap_, uint256 maxPerBlock_, uint256 killThreshold_) external onlyOwner {
        lpCap = lpCap_;
        maxPerBlock = maxPerBlock_;
        killThreshold = killThreshold_;
        emit RiskParamsUpdated(lpCap_, maxPerBlock_, killThreshold_);
    }

    /// @notice Emergency kill switch.
    function kill() external onlyOwner {
        killed = true;
    }

    /// @notice Recover from kill switch.
    function recover() external onlyOwner {
        killed = false;
        adverseStreak = 0;
    }

    // ── Reporting ────────────────────────────────────────────

    /// @notice Per-share APR estimate.
    /// @param rebateHistory Array of recent rebate amounts.
    /// @param periodDays Time period in days.
    function estimateApr(uint256[] calldata rebateHistory, uint256 periodDays) external view returns (uint256) {
        uint256 totalRebate;
        for (uint256 i = 0; i < rebateHistory.length; i++) {
            totalRebate += rebateHistory[i];
        }
        uint256 tvl = totalAssets();
        if (tvl == 0 || periodDays == 0) return 0;
        return (totalRebate * 365 * 1e4) / (tvl * periodDays);
    }

    /// @notice Current drawdown from peak.
    function drawdown() external view returns (uint256) {
        // Simplified: can be enhanced with historical peak tracking
        return 0;
    }
}
