use anchor_lang::prelude::*;

declare_id!("HVHLGn78m4vUQsDEZuK4jutYwq7W19xxXcSYR722PLu2");

#[error_code]
pub enum StakeError {
    #[msg("Invalid rate.")]
    InvalidRate,
    #[msg("Not in staking range.")]
    NotInStakingRange,
    #[msg("Redeem not cool down.")]
    RedeemNotCoolDown,
    #[msg("Invalid redeem amount.")]
    InvalidRedeemAmount,
    #[msg("Stake account was fronzen.")]
    StakeAccountFrozen,
    #[msg("Reach the staking limit")]
    StakingLimit,
}

const STAKING_KEY: &[u8] = b"staking";
const POOL_KEY: &[u8] = b"pool";
const RECEIPT_KEY: &[u8] = b"receipt";

#[program]
pub mod scale_staking {
    use super::*;

    use std::ops::Not;

    pub fn create(ctx: Context<Create>) -> Result<()> {
        let stake_account = &mut ctx.accounts.stake.load_init()?;
        stake_account.owner = ctx.accounts.owner.key().clone();
        stake_account.status = StakeStatus::Awailable;

        anchor_lang::solana_program::log::sol_log_compute_units();
        msg!("created stake account");
        Ok(())
    }

    pub fn add_staking_pool(
        ctx: Context<AddStakingPool>,
        duration: Duration,
        redeem_duration: Duration,
        profit_rate: Rate,
        stake_rate: Rate,
        redeem_rate: Rate,
        start: i64,
        funding: Option<u64>,
    ) -> Result<()> {
        if profit_rate.is_valid().not() || 
           redeem_rate.is_valid().not() {
            return Err(StakeError::InvalidRate.into());
        }
        let staking_pool = &mut ctx.accounts.staking_pool;
        staking_pool.initialized(
            ctx.accounts.stake.load()?.stakings,
            funding,
            duration,
            redeem_duration,
            profit_rate,
            stake_rate,
            redeem_rate,
            start,
            ctx.accounts.clock.unix_timestamp,
        );

        let stake_account = &mut ctx.accounts.stake.load_mut()?;
        stake_account.stakings += 1;

        anchor_lang::solana_program::log::sol_log_compute_units();
        msg!("created staking pool");
        Ok(())
    }

    pub fn freeze(ctx: Context<Freeze>) -> Result<()> {
        let staking_pool_account = &mut ctx.accounts.staking_pool;
        staking_pool_account.status = StakingPoolStatus::Frozen;

        anchor_lang::solana_program::log::sol_log_compute_units();
        msg!("frozen stake account");
        Ok(())
    }

    pub fn thaw(ctx: Context<Thaw>) -> Result<()> {
        let staking_pool_account = &mut ctx.accounts.staking_pool;
        staking_pool_account.status = StakingPoolStatus::Staking;

        anchor_lang::solana_program::log::sol_log_compute_units();
        msg!("thawed stake account");
        Ok(())
    }

    pub fn init_receipt(ctx: Context<InitReceipt>) -> Result<()> {
        let staking_receipt = &mut ctx.accounts.staking_receipt;
        staking_receipt.owner = ctx.accounts.payer.key().clone();
        staking_receipt.staking_pool = ctx.accounts.staking_pool.key().clone();

        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        let now = ctx.accounts.clock.unix_timestamp;
        let stake_account = &mut ctx.accounts.stake.load_mut()?;
        let staking_pool_account = &mut ctx.accounts.staking_pool;
        let staking_receipt = &mut ctx.accounts.staking_receipt;

        if now > staking_pool_account.stop_at() {
            return Err(StakeError::NotInStakingRange.into());
        }

        if staking_pool_account
            .is_stakable(staking_receipt.amount, amount)
            .not() {
            return Err(StakeError::StakingLimit.into());
        }

        staking_receipt.amount += amount;
        staking_pool_account.amount += amount;

        if let Some(funding) = staking_pool_account.funding {
            if staking_pool_account.status == StakingPoolStatus::Funding &&
               staking_pool_account.amount > funding {
                staking_pool_account.status = StakingPoolStatus::Staking;
                stake_account.amount += staking_pool_account.amount;
            }
        }

        if staking_pool_account.status == StakingPoolStatus::Staking {
            stake_account.amount += amount;
        }

        anchor_lang::solana_program::log::sol_log_compute_units();
        msg!("staking {}", amount);
        Ok(())
    }

    pub fn redeem(ctx: Context<Redeem>) -> Result<()> {
        let now = ctx.accounts.clock.unix_timestamp;
        let stake_account = &mut ctx.accounts.stake.load_mut()?;
        let staking_pool_account = &mut ctx.accounts.staking_pool;
        let staking_receipt = &mut ctx.accounts.staking_receipt;

        stake_account.amount -= staking_receipt.amount;
        stake_account.redeem += staking_receipt.amount;
        staking_pool_account.amount -= staking_receipt.amount;

        use StakingPoolStatus::*;
        if staking_pool_account.status == Staking &&
           now >= staking_pool_account.stop_at() {
            staking_pool_account.status = Redeeming;
        }

        match staking_pool_account.status {
            Staking => staking_receipt.redeemable_at = Some(now + staking_pool_account.redeem_duration.into_i64()),
            Redeeming => staking_receipt.redeemable_at = Some(now),
            _ => unreachable!(),
        }

        anchor_lang::solana_program::log::sol_log_compute_units();
        Ok(())
    }

    pub fn confirm_redeem(ctx: Context<ConfirmRedeem>) -> Result<()> {
        let now = ctx.accounts.clock.unix_timestamp;
        let stake_account = &mut ctx.accounts.stake.load_mut()?;
        let staking_receipt = &mut ctx.accounts.staking_receipt;
        if staking_receipt.is_redeemable(now).not() {
            return Err(StakeError::RedeemNotCoolDown.into());
        }

        stake_account.redeem -= staking_receipt.amount;
        staking_receipt.amount = 0;

        anchor_lang::solana_program::log::sol_log_compute_units();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, AnchorDeserialize, AnchorSerialize)]
pub enum StakeStatus {
    Uninitialized,
    Awailable,
}

#[derive(Debug, Clone, Copy, PartialEq, AnchorDeserialize, AnchorSerialize)]
pub enum StakingPoolStatus {
    Funding,
    Staking,
    Redeeming,
    Frozen,
}

#[derive(Debug, Clone, Copy, AnchorDeserialize, AnchorSerialize)]
#[repr(C)]
#[non_exhaustive]
pub enum Duration {
    OneHour,
    OneDay,
    OneWeek,
    OneMonth,
    OneYear,
}
impl Duration {
    pub fn into_i64(&self) -> i64 {
        match self {
            Duration::OneHour => 3600,
            Duration::OneDay => 86400,
            Duration::OneWeek => 604800,
            Duration::OneMonth => 2592000,
            Duration::OneYear => 31536000,
        }
    }
}

#[derive(Debug, Clone, Copy, AnchorDeserialize, AnchorSerialize)]
pub struct Rate {
    pub numerator: u32,
    pub denominator: u32,
}
impl Rate {
    pub fn is_valid(&self) -> bool {
        self.numerator > 0 
            && self.denominator > 0 
            && self.numerator <= self.denominator
    }
}

/// accounts definitions
#[account(zero_copy)]
pub struct StakeAccount {
    pub status: StakeStatus,
    /// market address.
    pub owner: Pubkey,
    pub stakings: u32,
    /// the stake amount of this market, for calculating profit or loss.
    /// if amount < pool_amount: is profit, stake / amount * pool_amount * profit_rate.
    /// else: is loss, stake / amount * pool_amount.
    pub amount: u64,
    pub redeem: u64,
}

impl StakeAccount {
    pub const LEN: usize = 1 + 32 + 4 + 8 + 8;
}

#[account]
pub struct StakingPool {
    pub status: StakingPoolStatus,
    pub id: u32,
    pub amount: u64,
    // target.
    pub funding: Option<u64>,
    pub created_at: i64,
    pub start: i64,
    pub duration: Duration,
    pub redeem_duration: Duration,
    pub profit_rate: Rate,
    pub stake_rate: Rate,
    pub redeem_rate: Rate,
}
impl StakingPool {
    pub const LEN: usize = 1 + 4 + 8 + 9 + 8 + 8 + 1 + 1 + 8 + 8 + 8;

    #[allow(clippy::too_many_arguments)]
    pub fn initialized(
        &mut self,
        id: u32,
        funding: Option<u64>,
        duration: Duration,
        redeem_duration: Duration,
        profit_rate: Rate,
        stake_rate: Rate,
        _redeem_rate: Rate,
        start: i64,
        now: i64,
    ) {
        self.id = id;
        self.funding = funding;
        self.status = if funding.is_some() {
            StakingPoolStatus::Funding
        } else {
            StakingPoolStatus::Staking
        };
        self.duration = duration;
        self.redeem_duration = redeem_duration;
        self.profit_rate = profit_rate;
        self.stake_rate = stake_rate;
        self.redeem_rate = Rate {
            numerator: 10000,
            denominator: 10000,
        };
        self.start = start;
        self.created_at = now;
    }

    pub fn stop_at(&self) -> i64 {
        match self.duration {
            Duration::OneHour => self.start + 3600,
            Duration::OneDay => self.start + 86400,
            Duration::OneWeek => self.start + 604800,
            Duration::OneMonth => self.start + 2592000,
            Duration::OneYear => self.start + 31536000,
        }
    }

    pub fn is_stakable(&self, origin: u64, stake_in: u64) -> bool {
        let left = stake_in
            .checked_add(origin).unwrap_or(u64::MAX)
            .checked_mul(self.stake_rate.denominator as u64).unwrap_or(u64::MAX);
        let right = self.amount
            .checked_add(stake_in).unwrap_or(0)
            .checked_mul(self.stake_rate.numerator as u64).unwrap_or(0);
        left <= right
    }
}

#[account]
pub struct StakingReceipt {
    pub owner: Pubkey,
    pub staking_pool: Pubkey,
    pub amount: u64,
    pub redeemable_at: Option<i64>,
    pub redeemable: Option<u64>,
}
impl StakingReceipt {
    pub const LEN: usize = 32 + 32 + 8 + 8 + 9;

    pub fn is_redeemable(&self, now: i64) -> bool {
        if let Some(time) = self.redeemable_at {
            return now >= time;
        }
        false
    }
}

#[derive(Accounts)]
pub struct Create<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: owner is the market account.
    pub owner: UncheckedAccount<'info>,
    #[account(init,
        seeds = [owner.key().as_ref(), STAKING_KEY],
        bump,
        payer = payer,
        space = 8 + StakeAccount::LEN,
    )]
    pub stake: AccountLoader<'info, StakeAccount>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AddStakingPool<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: market account.
    pub owner: UncheckedAccount<'info>,
    #[account(mut,
        seeds = [owner.key().as_ref(), STAKING_KEY],
        bump,
        constraint = stake.load()?.owner == owner.key(),
        constraint = stake.load()?.status == StakeStatus::Awailable,
    )]
    pub stake: AccountLoader<'info, StakeAccount>,
    #[account(init,
        seeds = [stake.key().as_ref(), stake.load()?.stakings.to_le_bytes().as_ref(), POOL_KEY],
        bump,
        payer = payer,
        space = 8 + StakingPool::LEN,
    )]
    pub staking_pool: Account<'info, StakingPool>,
    pub system_program: Program<'info, System>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts)]
pub struct Freeze<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: market account.
    pub owner: UncheckedAccount<'info>,
    #[account(
        seeds = [owner.key().as_ref(), STAKING_KEY],
        bump,
        constraint = stake.load()?.owner == owner.key(),
    )]
    pub stake: AccountLoader<'info, StakeAccount>,
    #[account(mut,
        seeds = [stake.key().as_ref(), stake.load()?.stakings.to_le_bytes().as_ref(), POOL_KEY],
        bump,
        constraint = staking_pool.status == StakingPoolStatus::Staking,
    )]
    pub staking_pool: Account<'info, StakingPool>,
}

#[derive(Accounts)]
pub struct Thaw<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: market account.
    pub owner: UncheckedAccount<'info>,
    #[account(
        seeds = [owner.key().as_ref(), STAKING_KEY],
        bump,
        constraint = stake.load()?.owner == owner.key(),
    )]
    pub stake: AccountLoader<'info, StakeAccount>,
    #[account(mut,
        seeds = [stake.key().as_ref(), stake.load()?.stakings.to_le_bytes().as_ref(), POOL_KEY],
        bump,
        constraint = staking_pool.status == StakingPoolStatus::Frozen,
    )]
    pub staking_pool: Account<'info, StakingPool>,
}

#[derive(Accounts)]
pub struct InitReceipt<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: market account.
    pub owner: UncheckedAccount<'info>,
    #[account(
        seeds = [owner.key().as_ref(), STAKING_KEY],
        bump,
        constraint = stake.load()?.owner == owner.key(),
        constraint = stake.load()?.status == StakeStatus::Awailable,
    )]
    pub stake: AccountLoader<'info, StakeAccount>,
    #[account(
        seeds = [stake.key().as_ref(), staking_pool.id.to_le_bytes().as_ref(), POOL_KEY],
        bump,
    )]
    pub staking_pool: Account<'info, StakingPool>,
    #[account(init,
        seeds = [staking_pool.key().as_ref(), payer.key().as_ref(), RECEIPT_KEY],
        bump,
        payer = payer,
        space = 8 + StakingReceipt::LEN,
    )]
    pub staking_receipt: Account<'info, StakingReceipt>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: market account.
    pub owner: UncheckedAccount<'info>,
    #[account(mut,
        seeds = [owner.key().as_ref(), STAKING_KEY],
        bump,
        constraint = stake.load()?.owner == owner.key(),
        constraint = stake.load()?.status == StakeStatus::Awailable,
    )]
    pub stake: AccountLoader<'info, StakeAccount>,
    #[account(mut,
        seeds = [stake.key().as_ref(), staking_pool.id.to_le_bytes().as_ref(), POOL_KEY],
        bump,
    )]
    pub staking_pool: Account<'info, StakingPool>,
    #[account(mut,
        seeds = [staking_pool.key().as_ref(), payer.key().as_ref(), RECEIPT_KEY],
        bump,
        constraint = staking_receipt.owner == payer.key(),
        constraint = staking_receipt.staking_pool == staking_pool.key(),
    )]
    pub staking_receipt: Account<'info, StakingReceipt>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts)]
pub struct Redeem<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: market account.
    pub owner: UncheckedAccount<'info>,
    #[account(mut,
        seeds = [owner.key().as_ref(), STAKING_KEY],
        bump,
        constraint = stake.load()?.owner == owner.key(),
    )]
    pub stake: AccountLoader<'info, StakeAccount>,
    #[account(mut,
        seeds = [stake.key().as_ref(), staking_pool.id.to_le_bytes().as_ref(), POOL_KEY],
        bump,
    )]
    pub staking_pool: Account<'info, StakingPool>,
    #[account(mut,
        seeds = [staking_pool.key().as_ref(), payer.key().as_ref(), RECEIPT_KEY],
        bump,
        constraint = staking_receipt.redeemable.is_none(),
    )]
    pub staking_receipt: Account<'info, StakingReceipt>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts)]
pub struct ConfirmRedeem<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: market account.
    pub owner: UncheckedAccount<'info>,
    #[account(mut,
        seeds = [owner.key().as_ref(), STAKING_KEY],
        bump,
        constraint = stake.load()?.owner == owner.key(),
    )]
    pub stake: AccountLoader<'info, StakeAccount>,
    #[account(
        seeds = [stake.key().as_ref(), staking_pool.id.to_le_bytes().as_ref(), POOL_KEY],
        bump,
    )]
    pub staking_pool: Account<'info, StakingPool>,
    #[account(mut,
        close = payer,
        seeds = [staking_pool.key().as_ref(), payer.key().as_ref(), RECEIPT_KEY],
        bump,
        constraint = staking_receipt.redeemable.is_some(),
    )]
    pub staking_receipt: Account<'info, StakingReceipt>,
    pub clock: Sysvar<'info, Clock>,
}