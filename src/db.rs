use std::sync::Arc;

use chrono::{DateTime, Utc};
use http_types::Url;
use rusqlite::Connection;

use crate::{Product, ProductInfo};

pub fn insert_new_product_info(
    conn: Arc<Connection>,
    name: &str,
    url: &Url,
    product: &ProductInfo,
) -> Result<(), anyhow::Error> {
    conn.execute(
        "INSERT INTO product_prices (name, url, timestamp, price) values (?1, ?2, ?3, ?4)",
        &[
            name,
            &url.to_string(),
            &product.time.to_rfc3339(),
            &product.price.to_string(),
        ],
    )?;
    Ok(())
}
