use std::sync::Arc;

use cdrs_tokio::{
    authenticators::NoneAuthenticator,
    cluster::{ClusterTcpConfig, NodeTcpConfigBuilder, TcpConnectionPool},
    frame::frame_response::ResponseBody,
};
use cdrs_tokio::{cluster::session, cluster::session::Session};
use cdrs_tokio::{load_balancing::SingleNode, query_values};
use cdrs_tokio::{query::QueryExecutor, types::IntoRustByName};
use chrono::{DateTime, Utc};
use reqwest::Url;
use rusqlite::Connection;

use crate::{Product, ProductInfo};

pub type ScyllaSession = Session<SingleNode<TcpConnectionPool<NoneAuthenticator>>>;

pub async fn new_session(addr: &str) -> Result<ScyllaSession, anyhow::Error> {
    let node = NodeTcpConfigBuilder::new(addr, NoneAuthenticator {}).build();
    let cluster_config = ClusterTcpConfig(vec![node]);
    Ok(session::new(&cluster_config, SingleNode::new()).await?)
}

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

pub async fn get_product_information(
    product_name: &str,
    session: &Session<SingleNode<TcpConnectionPool<NoneAuthenticator>>>,
) -> Result<Option<Vec<Product>>, anyhow::Error> {
    let query_values = query_values!(
        "name" => product_name
    );
    let query = r#"SELECT * FROM amazon.prices WHERE name = ?"#;
    let frame = session.query_with_values(query, query_values).await?;
    let response = ResponseBody::from(&frame.body, &frame.opcode)?;
    let rows = response.into_rows().unwrap();
    let mut products = Vec::new();
    for row in rows {
        let product = Product::from(row);
        products.push(product);
    }
    Ok(Some(products))
}

pub async fn get_product_prices(
    product_name: &str,
    session: &Session<SingleNode<TcpConnectionPool<NoneAuthenticator>>>,
) -> Result<Option<Vec<(String, DateTime<Utc>)>>, anyhow::Error> {
    let query_values = query_values!(
        "name" => product_name
    );
    let query = r#"SELECT price, time FROM amazon.prices WHERE name = ?"#;
    let frame = session.query_with_values(query, query_values).await?;
    let response = ResponseBody::from(&frame.body, &frame.opcode)?;
    let rows = response.into_rows().unwrap();
    let mut prices = Vec::new();
    for row in rows {
        let price = row.get_by_name("price").unwrap().unwrap();
        let time = row.get_by_name("time").unwrap().unwrap();
        prices.push((price, time));
    }
    Ok(Some(prices))
}

pub async fn get_all_products(
    session: &Session<SingleNode<TcpConnectionPool<NoneAuthenticator>>>,
) -> Result<Option<Vec<Product>>, anyhow::Error> {
    let query = r#"SELECT * FROM amazon.prices"#;
    let frame = session.query(query).await?;
    let response = ResponseBody::from(&frame.body, &frame.opcode)?;
    let rows = response.into_rows().unwrap();
    let mut products = Vec::new();
    for row in rows {
        let product = Product::from(row);
        products.push(product);
    }
    Ok(Some(products))
}
