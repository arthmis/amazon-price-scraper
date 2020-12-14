#![allow(warnings)]
use anyhow::{Context, Error, Result};
use async_std::{
    future::timeout,
    task::{self, JoinHandle},
};
use bollard::service::HostConfig;
use bollard::{
    container::{Config, CreateContainerOptions, StartContainerOptions},
    models::ContainerStateStatusEnum,
};
use chrono::Local;
use db::new_session;
use prettytable::Table;
use prettytable::{cell, row};
use scraper::{Html, Selector};
use task::sleep;
use textwrap::fill;

use std::io::{BufRead, BufReader};
use std::{fs::File, time::Duration};

use url::Url;

use cdrs_tokio::{
    authenticators::Authenticator,
    cluster::session::Session,
    consistency::Consistency,
    query::{Query, QueryParams},
    query_values,
};
use cdrs_tokio::{authenticators::NoneAuthenticator, cluster::TcpConnectionPool};
use cdrs_tokio::{cluster::session, load_balancing::LoadBalancingStrategy};
use cdrs_tokio::{
    cluster::{ClusterTcpConfig, NodeTcpConfigBuilder, TcpConnectionsManager},
    query::QueryExecutor,
};
use cdrs_tokio::{frame::traits::TryFromRow, types::rows::Row};
use cdrs_tokio::{load_balancing::RoundRobin, query::QueryValues};
use futures::future::Future;
use log::{debug, error, info, warn};
use reqwest::ClientBuilder;
use simplelog::{LevelFilter, WriteLogger};

pub mod db;
use db::*;
#[derive(Debug, Clone)]
struct Product {
    name: String,
    url: Url,
    time: chrono::DateTime<Local>,
    price: String,
}

impl TryFromRow for Product {
    fn try_from_row(row: Row) -> Result<Self, cdrs_tokio::Error> {
        dbg!(&row);
        Ok(Product {
            name: String::new(),
            price: String::new(),
            url: Url::parse("www.google.com").unwrap(),
            time: Local::now(),
        })
    }
}

fn init_logging() -> Result<(), Error> {
    let logger_config = {
        let mut builder = simplelog::ConfigBuilder::new();
        builder.set_time_to_local(true);
        builder.set_time_format_str("%r %d-%m-%Y");
        builder.add_filter_allow_str("amazon_price_scraper");
        builder.add_filter_allow_str("amazon_price_scraper::db");
        builder.build()
    };
    let log_file = {
        let mut options = std::fs::OpenOptions::new();
        options.append(true);
        options.create(true);
        options.open("amazon-price-scraper.log")?
    };
    let _ = WriteLogger::init(LevelFilter::Info, logger_config, log_file)?;
    Ok(())
}

#[async_std::main]
async fn main() -> Result<(), Error> {
    init_logging()?;

    // let mut urls: Vec<Url> = Vec::new();
    // let amazon_urls_file = File::open("amazon_product_urls.txt").expect("file not found");
    // for line in BufReader::new(amazon_urls_file).lines() {
    //     let possible_url = Url::parse(&line.expect("line couldn't be read"));
    //     match possible_url {
    //         Ok(url) => urls.push(url),
    //         Err(error) => println!("couldn't parse url: {}\n", error),
    //     }
    // }

    // let mut products: Vec<Product> = {
    //     // let products = timeout(Duration::from_secs(10), get_product_details(urls)).await??;
    //     // let products = get_products(&urls).await?;
    //     let products = get_products(&urls[..1]).await?;
    //     dbg!(&products);
    //     products
    // };

    // dbg!(&products);

    // let mut my_table = Table::new();
    // my_table.add_row(row!["Name", "Price"]);

    // for product in products {
    //     my_table.add_row(row![&fill(&product.name, 65), &fill(&product.price, 15)]);
    // }

    // // my_table.printstd();
    // info!("Table Data:\n{}", my_table.to_string());

    // TODO: if docker fails to connect then this should send a desktop notification
    // which should prompt me to start docker and allow the program to run correctly
    let docker = bollard::Docker::connect_with_local_defaults()?;

    let container_name = "scylla";
    match docker.inspect_container(container_name, None).await {
        // if found then start container
        Ok(res) => {
            // TODO: Container might already be running so handle error
            // in that case
            let state = res.state.unwrap();
            match state.status.unwrap() {
                ContainerStateStatusEnum::RUNNING => {}
                ContainerStateStatusEnum::PAUSED | ContainerStateStatusEnum::EXITED => {
                    docker
                        .start_container(
                            container_name,
                            Some(StartContainerOptions {
                                detach_keys: "ctrl-^",
                            }),
                        )
                        .await?;
                    info!("Starting {} container.", container_name);
                    sleep(Duration::from_secs(30)).await;
                }
                _ => return panic!("State should be running or paused"),
            }
        }
        // if err then create a container and start it
        Err(err) => {
            // log error first
            warn!("Potential issue finding container {}.", container_name);
            error!("{}", err);

            let host_config = HostConfig {
                memory: Some(2_000_000_000),
                ..Default::default()
            };
            let create_container_config: Config<String> = Config {
                host_config: Some(host_config),
                image: Some("scylladb/scylla".to_string()),
                ..Default::default()
            };

            let res = docker
                .create_container(
                    Some(CreateContainerOptions {
                        name: container_name,
                    }),
                    create_container_config,
                )
                .await
                .unwrap();

            info!("Creating {} container.", container_name);

            docker
                .start_container(
                    container_name,
                    Some(StartContainerOptions {
                        detach_keys: "ctrl-^",
                    }),
                )
                .await?;
            info!("Starting {} container.", container_name);
            sleep(Duration::from_secs(30)).await;
        }
    };
    // Here goes code to move product data into the database then program should stop container
    // and close docker down if that is possible

    // initialize scylla db
    let session = new_session("127.0.0.1:9042").await?;
    session.query(CREATE_KEYSPACE).await?;
    info!("Created Keyspace amazon.");

    session.query(CREATE_PRODUCT_TABLE).await?;
    info!("Created table to store product data.");

    // let product = products.pop().unwrap();
    let product = Product {
        name: String::from("Nikon z6"),
        url: Url::parse("https://google.com").unwrap(),
        time: Local::now(),
        price: String::from("$2000.00"),
    };

    let query_values = query_values!(
        "name" => product.name,
        "url" => product.url.to_string(),
        // "time" => product.time.format("%Y-%m-%d %H:%M:%S").to_string(),
        "time" => product.time.timestamp(),
        "price" => product.price
    );
    session
        .query_with_values(ADD_PRODUCT_PRICE, query_values)
        .await?;

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
        // client = client.user_agent(
        //     "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:84.0) Gecko/20100101 Firefox/84.0",
        // );
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
    let req = client.get(url.clone()).build()?;
    let time = chrono::Local::now();
    let res = client.execute(req).await?;
    let document = res.text_with_charset("utf-8").await?;

    let document = document;
    let document = Html::parse_document(&document);

    let price_selector =
        Selector::parse("#priceblock_ourprice").expect("couldn't parse css price id selector");
    let title_selector =
        Selector::parse("#productTitle").expect("couldn't parse css title id selector");
    let name = document
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

    let price = match prod_price {
        Some(price) => price.inner_html(),
        None => {
            let dealprice_selector =
                Selector::parse("#priceblock_dealprice").expect("couldn't parse css id selector");

            let deal_price = document.select(&dealprice_selector).next();
            match deal_price {
                Some(price) => price.inner_html(),
                None => "Sold Out".to_string(),
            }
        }
    };
    let mut product = Product {
        name,
        price,
        url,
        time,
    };
    Ok(product)
}
