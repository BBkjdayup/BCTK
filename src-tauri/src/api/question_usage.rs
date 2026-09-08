use sqlx::{QueryBuilder, Sqlite};
use time::{Date, Duration, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use super::models::{CommandError, CommandResult};

pub(super) const SEMESTER_UNUSED_DAYS: i64 = 180;
const MILLIS_PER_DAY: i64 = 86_400_000;

#[derive(Clone, Copy)]
pub(super) enum UsageConstraint {
    All,
    Never,
    Before(i64),
}

pub(super) fn validate_usage_filter(usage: &str) -> CommandResult<()> {
    if matches!(
        usage,
        "all"
            | "never"
            | "unused_this_semester"
            | "unused_this_month"
            | "unused_this_week"
            | "unused_today"
    ) {
        return Ok(());
    }
    Err(CommandError::validation("题目使用状态筛选无效。"))
}

fn timestamp_millis(value: OffsetDateTime) -> CommandResult<i64> {
    i64::try_from(value.unix_timestamp_nanos().div_euclid(1_000_000))
        .map_err(|_| CommandError::new("TIME_ERROR", "当前时间超出软件支持范围。"))
}

fn local_period_start_millis(usage: &str) -> CommandResult<i64> {
    let offset = UtcOffset::current_local_offset().map_err(|_| {
        CommandError::new("TIME_ERROR", "无法读取电脑本地时区，请检查系统时间设置。")
    })?;
    let now = OffsetDateTime::now_utc().to_offset(offset);
    let today = now.date();
    let start_date = match usage {
        "unused_today" => today,
        "unused_this_week" => today
            .checked_sub(Duration::days(i64::from(
                today.weekday().number_days_from_monday(),
            )))
            .ok_or_else(|| CommandError::new("TIME_ERROR", "无法计算本周起始时间。"))?,
        "unused_this_month" => Date::from_calendar_date(today.year(), today.month(), 1)
            .map_err(|_| CommandError::new("TIME_ERROR", "无法计算本月起始时间。"))?,
        _ => return Err(CommandError::validation("题目使用状态筛选无效。")),
    };
    timestamp_millis(PrimitiveDateTime::new(start_date, Time::MIDNIGHT).assume_offset(offset))
}

pub(super) fn resolve_usage_constraint(usage: &str) -> CommandResult<UsageConstraint> {
    validate_usage_filter(usage)?;
    match usage {
        "all" => Ok(UsageConstraint::All),
        "never" => Ok(UsageConstraint::Never),
        "unused_this_semester" => {
            let now = timestamp_millis(OffsetDateTime::now_utc())?;
            Ok(UsageConstraint::Before(now.saturating_sub(
                SEMESTER_UNUSED_DAYS.saturating_mul(MILLIS_PER_DAY),
            )))
        }
        "unused_this_month" | "unused_this_week" | "unused_today" => {
            Ok(UsageConstraint::Before(local_period_start_millis(usage)?))
        }
        _ => Err(CommandError::validation("题目使用状态筛选无效。")),
    }
}

pub(super) fn push_usage_constraint(
    builder: &mut QueryBuilder<'_, Sqlite>,
    usage_constraint: UsageConstraint,
) {
    match usage_constraint {
        UsageConstraint::All => {}
        UsageConstraint::Never => {
            builder.push(" AND q.last_used_at_ms IS NULL");
        }
        UsageConstraint::Before(cutoff) => {
            builder
                .push(" AND (q.last_used_at_ms IS NULL OR q.last_used_at_ms < ")
                .push_bind(cutoff)
                .push(")");
        }
    }
}
