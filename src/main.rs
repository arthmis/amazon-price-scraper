// #![allow(warnings)]
use anyhow::{bail, Context, Error, Result};
use async_std::task::{self, sleep};
use bollard::{
    container::{Config, CreateContainerOptions, StartContainerOptions},
    models::ContainerStateStatusEnum,
};
use bollard::{service::HostConfig, Docker};
use chrono::{DateTime, NaiveDateTime, Utc};

use clap::{App, Arg};
use plotters::prelude::{ChartBuilder, IntoDrawingArea, LabelAreaPosition, LineSeries, SVGBackend};
use plotters::style::{self, AsRelative, Color, Palette};
use prettytable::{cell, row, Table};
use rusqlite::{Connection, NO_PARAMS};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use scrape::{get_product_name, scrape_amazon};
use style::{Palette99, TextStyle};
use textwrap::fill;

use std::{sync::Arc, time::Duration};

use url::Url;

use cdrs_tokio::types::rows::Row;
use cdrs_tokio::types::IntoRustByName;
use log::{debug, error, info, warn};
use simplelog::{LevelFilter, WriteLogger};

pub mod db;
mod scrape;

use db::{new_session, ScyllaSession};
#[derive(Debug, Clone)]
pub struct Product {
    name: String,
    url: Url,
    time: chrono::DateTime<Utc>,
    price: ProductPrice,
}
#[derive(Debug, Clone)]
pub struct ProductInfo {
    price: ProductPrice,
    time: chrono::DateTime<Utc>,
}

const ADDR: &str = "127.0.0.1:9042";

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

impl From<Row> for ProductInfo {
    fn from(row: Row) -> Self {
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
        ProductInfo { time, price }
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
                .takes_value(true)
                .help("Removes a product from future price scraping"),
        )
        .arg(
            Arg::with_name("list")
            .short("-l")
            .long("list")
            .takes_value(false)
            .help("Lists all products that are currently scraped.")
        )
        .arg(
            Arg::with_name("plot")
            .short("-p")
            .long("plot")
            .takes_value(true)
            .help("Plots the price over time for a product.")
        );

    let matches = app.get_matches();
    if matches.is_present("scrape") {
        let _: Result<_, Error> = task::block_on(async {
            let conn = Connection::open("products.db")?;

            let mut stmt = conn.prepare("SELECT name, url FROM products")?;
            let mut products = Vec::new();
            let rows = stmt.query_map(NO_PARAMS, |row| Ok((row.get("name")?, row.get("url")?)))?;
            for info in rows {
                let (name, url): (String, String) = info?;
                products.push((name, Url::parse(&url)?));
            }
            // TODO: think about creating an iterator instead of creating a vector
            let urls = {
                let mut urls = Vec::new();
                for (_, url) in products.iter() {
                    urls.push(url.clone());
                }
                urls
            };

            let new_products_info = scrape_amazon(&urls);

            // TODO: if docker fails to connect then this should send a desktop notification
            // which should prompt me to start docker and allow the program to run correctly
            // let docker = Docker::connect_with_local_defaults()?;

            // let container_name = "scylla";
            // start_docker_container(&docker, container_name).await?;

            // Here goes code to move product data into the database then program should stop container
            // and close docker down if that is possible

            // initialize scylla db
            // let session = new_session(ADDR).await?;
            // session.query(db::CREATE_KEYSPACE).await?;
            // session.query(db::CREATE_PRODUCT_TABLE).await?;

            let new_products_info = new_products_info.await?;

            let conn = Arc::new(Connection::open("products.db")?);

            conn.execute(
                "
            CREATE TABLE IF NOT EXISTS product_prices(
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                url TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                price TEXT NOT NULL
            )",
                NO_PARAMS,
            )?;
            for ((name, url), new_info) in products.iter().zip(new_products_info.iter()) {
                db::insert_new_product_info(conn.clone(), name, url, new_info)?;
            }
            let mut product_table = Table::new();
            product_table.add_row(row!["Name", "Price"]);

            for ((name, _), product_info) in products.iter().zip(new_products_info.iter()) {
                product_table.add_row(row![
                    &fill(name, 65),
                    &fill(&product_info.price.to_string(), 15)
                ]);
            }
            product_table.printstd();
            Ok(())
        });
    } else if matches.is_present("product") {
        let url = matches.value_of_os("product").unwrap();
        let url = url.to_string_lossy().to_string();
        let db_url = url.clone();
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
                name TEXT NOT NULL,
                url TEXT NOT NULL
            )",
                NO_PARAMS,
            )?;

            conn.execute(
                "INSERT INTO Products (name, url) values (?1, ?2)",
                &[&name, &db_url],
            )?;
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
    } else if matches.is_present("plot") {
        let name = matches.value_of("plot").unwrap().to_owned();
        let product_data: Result<Vec<(String, DateTime<Utc>)>, Error> = task::block_on(async {
            let docker = Docker::connect_with_local_defaults()?;

            let container_name = "scylla";
            start_docker_container(&docker, container_name).await?;
            let session: ScyllaSession = new_session(ADDR).await?;
            let product_info = db::get_product_prices(&name, &session).await?.unwrap();
            dbg!(&product_info);

            Ok(product_info)
        });
        plot_data(&name, &product_data?)?;
    } else if matches.is_present("remove") {
        let name = matches.value_of("remove").unwrap().to_owned();

        let conn = Connection::open("products.db")?;

        let mut stmt = conn.prepare("DELETE FROM Products WHERE (name) = (?1)")?;
        stmt.execute(&[&name])?;

        let mut stmt = conn.prepare("DELETE FROM product_prices WHERE (name) = (?1)")?;
        stmt.execute(&[&name])?;
    }

    Ok(())
}

fn plot_data(name: &str, data: &[(String, DateTime<Utc>)]) -> Result<(), Error> {
    if data.is_empty() {
        bail!("The data for \"{}\" is empty. It cannot be plotted.", name);
    }
    let root = SVGBackend::new("plot.svg", (1920, 1080)).into_drawing_area();
    // root.fill(&WHITE)?;

    let x_axis = {
        let mut x = Vec::new();
        for (_, time) in data.iter() {
            x.push(*time);
        }
        x
    };
    let y_axis = {
        let mut y = Vec::new();
        for (price, _) in data.iter() {
            y.push(price.clone().parse::<Decimal>()?);
        }
        y
    };
    let x_min = *x_axis.iter().min().unwrap();
    let x_max = *x_axis.iter().max().unwrap();
    let y_max = *y_axis.iter().max().unwrap();
    let y_max = y_max.to_f64().unwrap() + y_max.to_f64().unwrap() / 2.0;

    let mut chart = ChartBuilder::on(&root)
        .caption(name, ("sans-serif", 5.percent_height()))
        .set_label_area_size(LabelAreaPosition::Left, 8.percent())
        .set_label_area_size(LabelAreaPosition::Bottom, 8.percent())
        .margin(5.percent())
        .build_cartesian_2d(x_min..x_max, 0.0..y_max)?;

    let text_style = TextStyle::from(("sans-serif", 18));
    chart
        .configure_mesh()
        .disable_mesh()
        .x_desc("Time")
        .y_desc("Price")
        .axis_desc_style(text_style)
        .draw()?;

    let line_series = LineSeries::new(
        x_axis
            .iter()
            .zip(y_axis.iter())
            .map(|(x, y)| (*x, y.to_f64().unwrap())),
        Palette99::pick(0).mix(0.9).stroke_width(3),
    );
    chart.draw_series(line_series)?;

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
        // handle errors beter
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
