// #![allow(warnings)]
use anyhow::{Context, Error, Result};
use async_std::task::{self, sleep};
use bollard::{
    container::{Config, CreateContainerOptions, StartContainerOptions},
    models::ContainerStateStatusEnum,
};
use bollard::{service::HostConfig, Docker};
use chrono::{DateTime, NaiveDateTime, Utc};

use clap::{App, Arg};
use rusqlite::{Connection, Rows, NO_PARAMS};
use scrape::{get_product_name, scrape_amazon};

use std::time::Duration;

use url::Url;

use cdrs_tokio::query::QueryExecutor;
use cdrs_tokio::types::rows::Row;
use cdrs_tokio::types::IntoRustByName;
use log::{debug, error, info, warn};
use simplelog::{LevelFilter, WriteLogger};

pub mod db;
mod scrape;

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

fn main() -> Result<(), Error> {
    init_logging()?;

    let app = App::new("Amazon Price Bot")
        .version("0.5")
        .author("Arthmis <arthmis20@gmail.com>")
        .arg(
            Arg::with_name("scrape")
                .long("scrape")
                .help("Immediately starts scraping Amazon, out of schedule.")
                .takes_value(false),
        )
        .arg(Arg::with_name("product").short("a").long("add").help(
            "Adds a product that will be scraped in future crawls. Takes a valid URL as input.",
        ).takes_value(true))
        .arg(
            Arg::with_name("remove")
                .short("r")
                .long("remove")
                .help("Removes a product from future price scraping"),
        )
        .arg(
            Arg::with_name("list")
            .short("-l")
            .long("list")
            .takes_value(false)
            .help("Lists all products that are currently scraped.")
        );

    let matches = app.get_matches();
    if matches.is_present("scrape") {
        let _: Result<_, Error> = task::block_on(async {
            let products = scrape_amazon();

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

            let products = products.await?;
            for product in products.iter() {
                db::insert_product(product, &session).await?;
            }
            Ok(())
        });
    // // my_table.printstd();
    // info!("Table Data:\n{}", my_table.to_string());

    // dbg!(db::get_all_products(&session).await?);
    } else if matches.is_present("product") {
        let url = matches.value_of_os("product").unwrap();
        let url = url.to_string_lossy().to_string();
        let url = Url::parse(&url)
            .with_context(|| format!("The provided url was not valid. Input url was: {}", url))?;
        // takes url and scrapes its web page to get its name
        let _: Result<(), Error> = task::block_on(async {
            let name = get_product_name(&url).await?;
            let conn = Connection::open("products.db")?;

            conn.execute(
                "
            CREATE TABLE IF NOT EXISTS Products (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL
            )",
                NO_PARAMS,
            )?;

            conn.execute("INSERT INTO Products (name) values (?1)", &[&name])?;
            Ok(())
        });
    // looks in sqlite database and retrieves all product names
    // prints them out
    } else if matches.is_present("list") {
        let conn = Connection::open("products.db")?;

        let mut stmt = conn.prepare("SELECT name FROM products")?;
        let rows = stmt.query_map(NO_PARAMS, |row| row.get("name"))?;
        for name in rows {
            let name: String = name?;
            println!("{}", name);
        }
    }

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
