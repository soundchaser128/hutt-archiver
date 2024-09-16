use chrono::NaiveDate;
use color_eyre::eyre::bail;
use tracing::info;

use crate::{DownloadContext, Result};

pub struct SetDatesArgs {
    pub start: String,
    pub end: String,
}

fn lerp_dates(start: NaiveDate, end: NaiveDate, percentage: f64) -> NaiveDate {
    let days = (end - start).num_days() as f64;
    let days = days * percentage;
    start + chrono::Duration::days(days as i64)
}

pub async fn run(context: DownloadContext, args: SetDatesArgs) -> Result<()> {
    let start_date = NaiveDate::parse_from_str(&args.start, "%Y-%m-%d")?;
    let end_date = NaiveDate::parse_from_str(&args.end, "%Y-%m-%d")?;

    if start_date > end_date {
        bail!("end date must be after start date.")
    }

    // interpolate start - end dates for all posts (just approximate)
    let all_posts = context.database.fetch_all().await?;
    let len = all_posts.len() as f64;
    for (index, post) in all_posts.into_iter().enumerate() {
        let percentage = index as f64 / len;
        let new_date = lerp_dates(start_date, end_date, percentage);
        info!("setting post {} to date {}", post.id, new_date);
        context.database.set_post_date(post.id, new_date).await?;
    }

    Ok(())
}
