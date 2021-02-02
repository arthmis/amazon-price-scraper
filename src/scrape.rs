use std::time::Duration;

use anyhow::Error;
use async_std::{future::timeout, task};
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use scraper::{Html, Selector};

use crate::{ProductInfo, ProductPrice};

pub async fn scrape_amazon(urls: &[Url]) -> Result<Vec<ProductInfo>, Error> {
    let products: Vec<ProductInfo> = {
        let products = scrape_products(urls).await?;
        products
    };

    Ok(products)
}

fn client() -> Result<Client, Error> {
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
    Ok(client.build()?)
}

async fn scrape_products(urls: &[Url]) -> Result<Vec<ProductInfo>, Error> {
    let mut products: Vec<ProductInfo> = Vec::new();

    let client = client()?;
    let now = Utc::now();
    for url in urls.iter() {
        let product = timeout(
            Duration::from_secs(5),
            task::spawn(scrape_product_price(url.clone(), client.clone(), now)),
        )
        .await?;
        products.push(product?);
    }
    Ok(products)
}

async fn scrape_product_price(
    url: Url,
    client: reqwest::Client,
    time: DateTime<Utc>,
) -> Result<ProductInfo, Error> {
    let req = client.get(url.clone()).build()?;
    let res = client.execute(req).await?;
    let document = res.text_with_charset("utf-8").await?;

    let document = document;
    let document = Html::parse_document(&document);

    let price_selector =
        Selector::parse("#priceblock_ourprice").expect("couldn't parse css price id selector");
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
    let product = ProductInfo { price, time };
    Ok(product)
}

pub async fn get_product_name(url: &Url) -> Result<String, Error> {
    let client = client()?;
    let req = client.get(url.clone()).build()?;
    let res = client.execute(req).await?;
    let document = res.text_with_charset("utf-8").await?;

    let document = document;
    let document = Html::parse_document(&document);

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

    Ok(name)
}
