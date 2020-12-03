#![allow(warnings)]
use anyhow::{Context, Error, Result};
use async_std::{future::timeout, task::JoinHandle};
use prettytable::Table;
use prettytable::{cell, row};
use scraper::{Html, Selector};
use textwrap::fill;

use futures::{future::join_all, Future};
use std::io::{BufRead, BufReader};
use std::{fs::File, time::Duration};

use url::Url;

use cdrs_tokio::load_balancing::RoundRobin;
use cdrs_tokio::{authenticators::Authenticator, cluster::session::Session};
use cdrs_tokio::{authenticators::NoneAuthenticator, cluster::TcpConnectionPool};
use cdrs_tokio::{cluster::session, load_balancing::LoadBalancingStrategy};
use cdrs_tokio::{
    cluster::{ClusterTcpConfig, NodeTcpConfigBuilder, TcpConnectionsManager},
    query::QueryExecutor,
};

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
        let products = get_products(urls).await?;
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

async fn get_products(urls: Vec<Url>) -> Result<Vec<Product>, Error> {
    let mut products: Vec<Product> = Vec::new();

    for url in urls.iter() {
        let product = async_std::task::spawn(get_product_detail(url.clone())).await?;
        products.push(product);
    }
    Ok(products)
}

async fn get_product_detail(url: Url) -> Result<Product, Error> {
    let document = surf::get(url.clone()).recv_string().await.unwrap();

    let document = document;
    dbg!(&document);
    // let document = Html::parse_document(&document?);
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
