//! Pemakaian strategi oleh user (miliknya sendiri, atau strategi Store yang sudah dibeli/trial):
//! - SCREENER: mencari coin dengan skor tertinggi — hanya strategi cakupan semua coin (`ALL`);
//! - BOT: robot order otomatis di akun DEMO atau LIVE (boleh keduanya = 2 pemakaian), memakai
//!   maksimal `capital_percent`% saldo akun. Dijalankan worker `strategy-bot` tiap hari.
//!
//! Terkunci ke 1 versi: versi baru dari pembuat strategi tidak otomatis dipakai sampai user
//! memperbarui (`upgrade_to_latest`), jadi perilaku bot tidak berubah diam-diam.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use super::model::StrategySubscription;
use super::repository::{
    create_subscription, exists_subscription, find_subscription_of_user, find_subscriptions_of_user,
    soft_delete_subscription, update_subscription, NewSubscription, SubscriptionUpdate,
};
use super::types::{CreateSubscriptionInput, SubscriptionDetail, SubscriptionPerformance, UpdateSubscriptionInput};
use crate::database::{begin, commit};
use crate::services::strategies::model::Strategy;
use crate::services::strategies::repository::find_strategies_by_ids;
use crate::services::strategies::service::{run_strategy_version, usable_strategy_in_tx};
use crate::services::strategies::types::{RunQuery, StrategyRun};
use crate::services::strategy_versions::model::StrategyVersion;
use crate::services::strategy_versions::repository::{find_version_by_id, find_versions_by_ids};
use crate::services::strategy_versions::types::BotConfig;
use crate::services::transactions::repository::strategy_performance_of_subscription;
use crate::services::users::repository::find_user_by_id;
use crate::strategy_engine::inputs;
use crate::utils::app_error::AppError;

const USAGES: [&str; 2] = ["SCREENER", "BOT"];
const ACCOUNT_MODES: [&str; 2] = ["DEMO", "LIVE"];
const MAX_SUBSCRIPTIONS_PER_USER: usize = 30;

fn not_found() -> AppError {
    AppError::NotFound("Pemakaian strategi tidak ditemukan".to_string())
}

fn bad(message: &str) -> AppError {
    AppError::BadRequest(message.to_string())
}

fn parse_usage(value: Option<&str>) -> Result<String, AppError> {
    let usage = value.map(|v| v.trim().to_uppercase()).unwrap_or_else(|| "SCREENER".to_string());
    if !USAGES.contains(&usage.as_str()) {
        return Err(bad("usage harus SCREENER atau BOT"));
    }
    Ok(usage)
}

fn parse_capital_percent(value: Option<Decimal>) -> Result<Decimal, AppError> {
    let percent = value
        .ok_or_else(|| bad("capital_percent wajib diisi untuk BOT — persen saldo yang boleh dipakai robot (1–100)"))?
        .round_dp(2);
    if percent < Decimal::ONE || percent > Decimal::ONE_HUNDRED {
        return Err(bad("capital_percent harus 1–100"));
    }
    Ok(percent)
}

/// Versi boleh dipakai sesuai jenis pemakaiannya: screener hanya untuk strategi semua coin.
fn check_usage(usage: &str, version: &StrategyVersion) -> Result<(), AppError> {
    if usage == "SCREENER" && !BotConfig::of(version).all_coins() {
        return Err(bad("Strategi untuk coin tertentu hanya bisa dipakai sebagai BOT, bukan SCREENER"));
    }
    Ok(())
}

/// Nilai setelan untuk 1 versi (dilengkapi default & dicek batasnya).
fn resolve(version: &StrategyVersion, values: &Value) -> Result<Value, AppError> {
    let specs = inputs::parse_specs(&version.inputs).map_err(AppError::BadRequest)?;
    inputs::resolve_values(&specs, values).map_err(AppError::BadRequest)
}

fn detail(
    subscription: StrategySubscription,
    strategy: Option<&Strategy>,
    version: Option<&StrategyVersion>,
    latest: Option<&StrategyVersion>,
) -> SubscriptionDetail {
    let version_no = version.map(|v| v.version_no).unwrap_or_default();
    SubscriptionDetail {
        strategy_name: strategy.map(|s| s.name.clone()).unwrap_or_default(),
        version_no,
        latest_version_no: latest.map(|v| v.version_no).filter(|no| *no > version_no),
        inputs: version.map(|v| v.inputs.clone()).unwrap_or(Value::Array(Vec::new())),
        bot: version.map(BotConfig::of),
        subscription,
    }
}

pub async fn create_subscription_service(
    pool: &PgPool,
    user_id: Uuid,
    actor: &str,
    input: CreateSubscriptionInput,
) -> Result<SubscriptionDetail, AppError> {
    let usage = parse_usage(input.usage.as_deref())?;
    let (account_mode, capital_percent) = if usage == "BOT" {
        let mode = input
            .account_mode
            .as_deref()
            .map(|m| m.trim().to_uppercase())
            .filter(|m| ACCOUNT_MODES.contains(&m.as_str()))
            .ok_or_else(|| bad("account_mode wajib DEMO atau LIVE untuk BOT"))?;
        (Some(mode), Some(parse_capital_percent(input.capital_percent)?))
    } else {
        (None, None)
    };

    let mut tx = begin(pool).await?;
    let (strategy, version) = usable_strategy_in_tx(&mut tx, input.strategy_id, user_id).await?;
    check_usage(&usage, &version)?;
    if account_mode.as_deref() == Some("LIVE") {
        let user = find_user_by_id(&mut tx, user_id).await?.ok_or_else(|| AppError::Unauthorized("User tidak ditemukan".to_string()))?;
        if user.api_key.is_none() || user.api_secret.is_none() {
            return Err(bad("Bot LIVE butuh API key & secret Binance — isi dulu di profil"));
        }
    }
    if find_subscriptions_of_user(&mut tx, user_id).await?.len() >= MAX_SUBSCRIPTIONS_PER_USER {
        return Err(AppError::BadRequest(format!(
            "Maksimal {MAX_SUBSCRIPTIONS_PER_USER} strategi yang dipakai — hentikan yang tidak dipakai"
        )));
    }
    if exists_subscription(&mut tx, user_id, strategy.id, &usage, account_mode.as_deref()).await? {
        let label = account_mode.as_deref().map(|m| format!("{usage} {m}")).unwrap_or(usage.clone());
        return Err(AppError::Conflict(format!("Kamu sudah memakai strategi ini sebagai {label}")));
    }
    let values = resolve(&version, &input.input_values)?;
    let subscription = create_subscription(
        &mut tx,
        &NewSubscription {
            user_id,
            strategy_id: strategy.id,
            strategy_version_id: version.id,
            usage: &usage,
            account_mode: account_mode.as_deref(),
            capital_percent,
            input_values: &values,
        },
        actor,
    )
    .await?;
    commit(tx).await?;
    Ok(detail(subscription, Some(&strategy), Some(&version), Some(&version)))
}

pub async fn get_subscriptions_service(pool: &PgPool, user_id: Uuid) -> Result<Vec<SubscriptionDetail>, AppError> {
    let mut tx = begin(pool).await?;
    let subscriptions = find_subscriptions_of_user(&mut tx, user_id).await?;
    let strategy_ids: Vec<Uuid> = subscriptions.iter().map(|s| s.strategy_id).collect();
    let strategies: HashMap<Uuid, Strategy> =
        find_strategies_by_ids(&mut tx, &strategy_ids).await?.into_iter().map(|s| (s.id, s)).collect();
    let mut version_ids: Vec<Uuid> = subscriptions.iter().map(|s| s.strategy_version_id).collect();
    version_ids.extend(strategies.values().filter_map(|s| s.current_version_id));
    let versions: HashMap<Uuid, StrategyVersion> =
        find_versions_by_ids(&mut tx, &version_ids).await?.into_iter().map(|v| (v.id, v)).collect();
    commit(tx).await?;

    Ok(subscriptions
        .into_iter()
        .map(|subscription| {
            let strategy = strategies.get(&subscription.strategy_id);
            let version = versions.get(&subscription.strategy_version_id);
            let latest = strategy.and_then(|s| s.current_version_id).and_then(|id| versions.get(&id));
            detail(subscription, strategy, version, latest)
        })
        .collect())
}

/// Ubah setelan / persen modal / aktif-nonaktif / pindah ke versi terbaru strategi.
pub async fn update_subscription_service(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    actor: &str,
    input: UpdateSubscriptionInput,
) -> Result<SubscriptionDetail, AppError> {
    let mut tx = begin(pool).await?;
    let existing = find_subscription_of_user(&mut tx, id, user_id).await?.ok_or_else(not_found)?;
    let (strategy, latest) = usable_strategy_in_tx(&mut tx, existing.strategy_id, user_id).await?;
    let version = if input.upgrade_to_latest {
        latest.clone()
    } else {
        find_version_by_id(&mut tx, existing.strategy_version_id).await?.ok_or_else(not_found)?
    };
    check_usage(&existing.usage, &version)?;
    // Setelan lama dicek ulang terhadap versi tujuan — deklarasi setelan bisa berubah antar versi.
    let values = resolve(&version, input.input_values.as_ref().unwrap_or(&existing.input_values))?;
    let capital_percent = match (existing.usage.as_str(), input.capital_percent) {
        ("BOT", Some(percent)) => Some(parse_capital_percent(Some(percent))?),
        ("BOT", None) => existing.capital_percent,
        _ => None,
    };

    let subscription = update_subscription(
        &mut tx,
        id,
        &SubscriptionUpdate {
            strategy_version_id: version.id,
            input_values: &values,
            capital_percent,
            is_active: input.is_active.unwrap_or(existing.is_active),
        },
        actor,
    )
    .await?;
    commit(tx).await?;
    Ok(detail(subscription, Some(&strategy), Some(&version), Some(&latest)))
}

/// Berhenti memakai strategi. Untuk BOT: posisi yang masih terbuka TIDAK dijual otomatis — tetap
/// tercatat & bisa dijual manual.
pub async fn delete_subscription_service(pool: &PgPool, id: Uuid, user_id: Uuid, actor: &str) -> Result<(), AppError> {
    let mut tx = begin(pool).await?;
    find_subscription_of_user(&mut tx, id, user_id).await?.ok_or_else(not_found)?;
    soft_delete_subscription(&mut tx, id, actor).await?;
    commit(tx).await
}

/// Jalankan strategi yang dipakai (versi & setelan milik pemakaian ini) ke coin cakupannya —
/// untuk BOT = sinyal yang akan dipakai robot.
pub async fn get_subscription_results_service(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    query: RunQuery,
) -> Result<StrategyRun, AppError> {
    let (subscription, version) = {
        let mut tx = begin(pool).await?;
        let subscription = find_subscription_of_user(&mut tx, id, user_id).await?.ok_or_else(not_found)?;
        // Strategi yang diarsipkan / hak pakainya habis tidak bisa dijalankan lagi.
        usable_strategy_in_tx(&mut tx, subscription.strategy_id, user_id).await?;
        let version = find_version_by_id(&mut tx, subscription.strategy_version_id)
            .await?
            .ok_or_else(not_found)?;
        commit(tx).await?;
        (subscription, version)
    };
    run_strategy_version(pool, &version, &subscription.input_values, &query).await
}

/// Performa asli bot ini dari transaksinya (DEMO/LIVE dipisah).
pub async fn get_subscription_performance_service(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
) -> Result<SubscriptionPerformance, AppError> {
    let mut tx = begin(pool).await?;
    find_subscription_of_user(&mut tx, id, user_id).await?.ok_or_else(not_found)?;
    let rows = strategy_performance_of_subscription(&mut tx, id).await?;
    commit(tx).await?;
    Ok(SubscriptionPerformance { subscription_id: id, performance: rows.into_iter().map(Into::into).collect() })
}
