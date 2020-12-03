#![allow(warnings)]
use anyhow::{Context, Error, Result};
use async_std::{future::timeout, task::JoinHandle};
use prettytable::Table;
use prettytable::{cell, row};
use scraper::{Html, Selector};
use textwrap::fill;

use std::fs::File;
use std::io::{BufRead, BufReader};

use url::Url;

use cdrs_tokio::load_balancing::RoundRobin;
use cdrs_tokio::{authenticators::Authenticator, cluster::session::Session};
use cdrs_tokio::{authenticators::NoneAuthenticator, cluster::TcpConnectionPool};
use cdrs_tokio::{cluster::session, load_balancing::LoadBalancingStrategy};
use cdrs_tokio::{
    cluster::{ClusterTcpConfig, NodeTcpConfigBuilder, TcpConnectionsManager},
    query::QueryExecutor,
};
use reqwest::ClientBuilder;

pub mod db;
#[derive(Debug, Clone)]
struct Product {
    title: String,
    price: String,
}
#[async_std::main]
async fn main() -> Result<(), Error> {
    let mut urls: Vec<Url> = Vec::new();
    let amazon_urls_file = File::open("amazon_product_urls.txt").expect("file not found");
    for line in BufReader::new(amazon_urls_file).lines() {
        let possible_url = Url::parse(&line.expect("line couldn't be read"));
        match possible_url {
            Ok(url) => urls.push(url),
            Err(error) => println!("couldn't parse url: {}\n", error),
        }
    }

    let products: Vec<Product> = {
        // let products = timeout(Duration::from_secs(10), get_product_details(urls)).await??;
        // let products = get_products(&urls).await?;
        let products = get_products(&urls[..1]).await?;
        dbg!(&products);
        products
    };

    let create_ks = "CREATE KEYSPACE IF NOT EXISTS amazon WITH REPLICATION = { \
                             'class' : 'SimpleStrategy', 'replication_factor' : 1 };";

    let result: Result<(), anyhow::Error> = {
        let node = NodeTcpConfigBuilder::new("127.0.0.1:9042", NoneAuthenticator {}).build();
        let cluster_config = ClusterTcpConfig(vec![node]);
        let session = session::new(&cluster_config, RoundRobin::new()).await?;
        session
            .query(create_ks)
            .await
            .expect("Keyspace create error");
        Ok(())
    };
    result.unwrap();

    dbg!(&products);

    let mut my_table = Table::new();
    my_table.add_row(row!["Title", "Price"]);

    for product in products {
        my_table.add_row(row![&fill(&product.title, 65), &fill(&product.price, 15)]);
    }

    my_table.printstd();
    Ok(())
}

async fn get_products(urls: &[Url]) -> Result<Vec<Product>, Error> {
    let mut products: Vec<Product> = Vec::new();

    for url in urls.iter() {
        let product = async_std::task::spawn(get_product_detail(url.clone())).await?;
        products.push(product);
    }
    Ok(products)
}

async fn get_product_detail(url: Url) -> Result<Product, Error> {
    let client = {
        let mut client = reqwest::ClientBuilder::new();
        client = client.user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:84.0) Gecko/20100101 Firefox/84.0",
        );
        let headers = {
            use reqwest::header;
            use reqwest::header::HeaderValue;
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                header::ACCEPT,
                HeaderValue::from_str(
                    "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
                )?,
            );
            headers.insert(
                header::ACCEPT_ENCODING,
                HeaderValue::from_str("gzip, deflate, br")?,
            );
            headers.insert(
                header::ACCEPT_LANGUAGE,
                HeaderValue::from_str("en-US,en;q=0.5")?,
            );
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_str("no-cache")?);
            headers.insert(header::CONNECTION, HeaderValue::from_str("keep-alive")?);
            headers.insert(header::DNT, HeaderValue::from_str("1")?);
            headers.insert(header::HOST, HeaderValue::from_str("www.amazon.com")?);
            headers.insert(header::PRAGMA, HeaderValue::from_str("no-cache")?);
            headers.insert(
                header::UPGRADE_INSECURE_REQUESTS,
                HeaderValue::from_str("1")?,
            );
            headers
        };
        client = client.gzip(true);
        client = client.brotli(true);
        client = client.cookie_store(true);
        client = client.default_headers(headers);
        client = client.use_native_tls();
        client.build()?
    };
    let req = client.get(url).build()?;
    let res = client.execute(req).await?;
    let document = res.text_with_charset("utf-8").await?;

    let document = document;
    let document = Html::parse_document(&document);

    let price_selector =
        Selector::parse("#priceblock_ourprice").expect("couldn't parse css price id selector");
    let title_selector =
        Selector::parse("#productTitle").expect("couldn't parse css title id selector");
    let mut product = Product {
        title: String::new(),
        price: String::new(),
    };
    product.title = document
        .select(&title_selector)
        .next()
        .expect("there is no title")
        .inner_html()
        .trim()
        .to_string()
        .split(',')
        .next()
        .unwrap()
        .to_string();
    let prod_price = document.select(&price_selector).next();

    match prod_price {
        Some(price) => {
            product.price = price.inner_html();
        }
        None => {
            let dealprice_selector =
                Selector::parse("#priceblock_dealprice").expect("couldn't parse css id selector");

            let deal_price = document.select(&dealprice_selector).next();
            match deal_price {
                Some(price) => product.price = price.inner_html(),
                None => product.price = "Sold Out".to_string(),
            };
        }
    }
    Ok(product)
}
