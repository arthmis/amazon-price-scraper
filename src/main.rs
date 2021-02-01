// #![allow(warnings)]
use anyhow::{Context, Error, Result};
use async_std::{future::timeout, task, task::sleep};
use bollard::{
    container::{Config, CreateContainerOptions, StartContainerOptions},
    models::ContainerStateStatusEnum,
};
use bollard::{service::HostConfig, Docker};
use chrono::{DateTime, NaiveDateTime, Utc};
use prettytable::Table;
use prettytable::{cell, row};
use scraper::{Html, Selector};
use textwrap::fill;

use std::io::{BufRead, BufReader};
use std::{fs::File, time::Duration};

use url::Url;

use cdrs_tokio::query::QueryExecutor;
use cdrs_tokio::types::rows::Row;
use cdrs_tokio::types::IntoRustByName;
use log::{debug, error, info, warn};
use simplelog::{LevelFilter, WriteLogger};

pub mod db;

use db::{get_products, new_session};
#[derive(Debug, Clone)]
pub struct Product {
    name: String,
    url: Url,
    time: chrono::DateTime<Utc>,
    price: ProductPrice,
}

impl From<Row> for Product {
    fn from(row: Row) -> Self {
        let name: String = row.get_by_name("name").unwrap().unwrap();
        let price = {
            let price: String = row.get_by_name("price").unwrap().unwrap();
            match price.as_str() {
                "Sold Out" => ProductPrice::SoldOut,
                _ => ProductPrice::Price(price),
            }
        };
        let time = {
            let time: i64 = row.get_by_name("time").unwrap().unwrap();
            let new_time = NaiveDateTime::from_timestamp(time, 0);
            DateTime::<Utc>::from_utc(new_time, Utc)
        };
        let url = {
            let raw_url: String = row.get_by_name("url").unwrap().unwrap();
            Url::parse(&raw_url).unwrap()
        };
        Product {
            name,
            url,
            time,
            price,
        }
    }
}

#[derive(Debug, Clone)]
enum ProductPrice {
    Price(String),
    SoldOut,
}

use std::string::ToString;
impl ToString for ProductPrice {
    fn to_string(&self) -> String {
        match self {
            Self::Price(price) => {
                dbg!(&price);
                let price = price.trim();
                price.replace('$', "")
            }
            Self::SoldOut => "Sold Out".to_string(),
        }
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

    let mut urls: Vec<Url> = Vec::new();
    let amazon_urls_file = File::open("amazon_product_urls.txt").expect("file not found");
    for line in BufReader::new(amazon_urls_file).lines() {
        let possible_url = Url::parse(&line.expect("line couldn't be read"));
        urls.push(possible_url?);
    }

    let products: Vec<Product> = {
        let products = scrape_products(&urls[..3]).await?;
        products
    };

    let mut my_table = Table::new();
    my_table.add_row(row!["Name", "Price"]);

    for product in products.iter() {
        my_table.add_row(row![
            &fill(&product.name, 65),
            &fill(&product.price.to_string(), 15)
        ]);
    }

    // // my_table.printstd();
    // info!("Table Data:\n{}", my_table.to_string());

    // TODO: if docker fails to connect then this should send a desktop notification
    // which should prompt me to start docker and allow the program to run correctly
    let docker = Docker::connect_with_local_defaults()?;

    let container_name = "scylla";
    start_docker_container(&docker, container_name).await?;

    // Here goes code to move product data into the database then program should stop container
    // and close docker down if that is possible

    // initialize scylla db
    let session = new_session("127.0.0.1:9042").await?;
    session.query(db::CREATE_KEYSPACE).await?;

    session.query(db::CREATE_PRODUCT_TABLE).await?;

    for product in products.iter() {
        db::insert_product(product, &session).await?;
    }
    // dbg!(db::get_all_products(&session).await?);

    Ok(())
}

async fn start_docker_container(
    docker: &Docker,
    container_name: &str,
) -> Result<(), bollard::errors::Error> {
    match docker.inspect_container(container_name, None).await {
        // if found then start container
        Ok(res) => {
            // TODO: Container might already be running so handle error
            // in that case
            let state = res.state.unwrap();
            match state.status.unwrap() {
                ContainerStateStatusEnum::RUNNING => {
                    debug!(
                        "Container is already running. Container state is: {}",
                        state.status.unwrap()
                    );
                }
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
                    // wait for container and scylla to start
                    sleep(Duration::from_secs(30)).await;
                }
                // I think this should be unreachable
                _ => {
                    error!(
                        "reached an unreachable state: Container State -> {:?}",
                        state
                    );
                    unreachable!("State should be running or paused");
                }
            }
        }
        // if err then create a container and start it
        Err(err) => {
            // log error first
            warn!("Potential issue finding container {}.", container_name);
            error!("Docker is possibly not currently running. Be sure to start docker before running this program: {}", err);

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
                .await?;

            debug!("Container create response: {:?}\n", res);
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
    Ok(())
}

async fn scrape_products(urls: &[Url]) -> Result<Vec<Product>, Error> {
    let mut products: Vec<Product> = Vec::new();

    let client = {
        let mut client = reqwest::ClientBuilder::new();
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
    for url in urls.iter() {
        let product = timeout(
            Duration::from_secs(5),
            task::spawn(scrape_product_detail(url.clone(), client.clone())),
        )
        .await?;
        products.push(product?);
    }
    Ok(products)
}

async fn scrape_product_detail(url: Url, client: reqwest::Client) -> Result<Product, Error> {
    let req = client.get(url.clone()).build()?;
    let time = Utc::now();
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
        Some(price) => ProductPrice::Price(price.inner_html()),
        None => {
            let dealprice_selector =
                Selector::parse("#priceblock_dealprice").expect("couldn't parse css id selector");

            let deal_price = document.select(&dealprice_selector).next();
            match deal_price {
                Some(price) => ProductPrice::Price(price.inner_html()),
                None => ProductPrice::SoldOut,
            }
        }
    };
    let product = Product {
        name,
        price,
        url,
        time,
    };
    Ok(product)
}
