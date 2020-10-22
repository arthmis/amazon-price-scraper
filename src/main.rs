use anyhow::{Context, Error, Result};
use prettytable::Table;
use prettytable::{cell, row};
use scraper::{Html, Selector};
use textwrap::fill;

use std::fs::File;
use std::io::{BufRead, BufReader};

use url::Url;

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

    let products = get_product_details(urls).await?;

    let mut my_table = Table::new();
    my_table.add_row(row!["Title", "Price"]);

    for product in products {
        my_table.add_row(row![&fill(&product.title, 65), &fill(&product.price, 15)]);
    }

    my_table.printstd();
    Ok(())
}

async fn get_product_details(urls: Vec<Url>) -> Result<Vec<Product>, Error> {
    let mut products: Vec<Product> = Vec::new();

    for url in urls {
        let html = reqwest::get(url.as_str())
            .await
            .with_context(|| "couldn't get website")?
            .text()
            .await
            .with_context(|| "couldn't convert html to string")?;
        let document = Html::parse_document(&html);
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
                let dealprice_selector = Selector::parse("#priceblock_dealprice")
                    .expect("couldn't parse css id selector");

                let deal_price = document.select(&dealprice_selector).next();
                match deal_price {
                    Some(price) => product.price = price.inner_html(),
                    None => product.price = "Sold Out".to_string(),
                };
            }
        };
        products.push(product);
    }
    Ok(products)
}
