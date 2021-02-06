use anyhow::Error;
use chrono::{DateTime, Utc};
use headers::{ACCEPT_LANGUAGE, CACHE_CONTROL, CONNECTION, CONTENT_ENCODING, COOKIE, HOST, PRAGMA};
use http_types::headers::{self, ACCEPT, ACCEPT_ENCODING, USER_AGENT};
use scraper::{Html, Selector};
use ureq::{Agent, Request};
use url::Url;

use crate::{ProductInfo, ProductPrice};

fn set_headers(req: Request) -> Request {
    req.set(
        ACCEPT.as_str(),
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
    )
    .set(ACCEPT_ENCODING.as_str(), "gzip, deflate, br")
    .set(ACCEPT_LANGUAGE.as_str(), "en-US,en;q=0.5")
    .set(CACHE_CONTROL.as_str(), "no-cache")
    .set(CONNECTION.as_str(), "keep-alive")
    .set(HOST.as_str(), "www.amazon.com")
    .set(PRAGMA.as_str(), "no-cache")
    .set(
        USER_AGENT.as_str(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:86.0) Gecko/20100101 Firefox/86.0",
        // "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/88.0.4324.146 Safari/537.36",
    )
    .set(CONTENT_ENCODING.as_str(), "gzip")
    .set("DNT", "1")
    .set("TE", "Trailers")
    .set("Upgrade-Insecure-Requests", "1")
    // .set(COOKIE.as_str(), "session-id=133-1434407-8387630; session-id-time=2082787201l; ubid-main=134-1085981-1294228; i18n-prefs=USD; session-token=aDNNWpqhuj0lVRZyMkuREjEl9xO0u9xTpvmtkORZi+0VmmKVqTpdInLmg1gwIHSGXtrKusLHSvJw+wusbTHVYnHKL6oXpmLIBDnfHSTzFrmOMSCsNt4A2hgjgw9LY7sizqYiNcYD8aMFylZzrofd9qoi6gdyq2Xfs3zBGiXOTSV38kg4lnEvZ8GYXjtNpJQz; csm-hit=tb:WSQV0YZMJP4ET1SZQ1VH+s-WSQV0YZMJP4ET1SZQ1VH|1612568046536&adb:adblk_no&t:1612568046536")
    // let headers = {
    //     // headers.insert(
    //     //     header::UPGRADE_INSECURE_REQUESTS,
    //     //     HeaderValue::from_str("1")?,
    //     // );
    //     );
    //     headers
    // };
    // client = client.gzip(true);
    // client = client.brotli(true);
    // client = client.cookie_store(true);
    // client = client.default_headers(headers);
    // client = client.use_native_tls();
    // client = client.https_only(true);
    // Ok(client.build()?)
}

pub fn scrape_products(urls: &[Url]) -> Result<Vec<ProductInfo>, Error> {
    let mut products: Vec<ProductInfo> = Vec::new();

    let agent = ureq::agent();
    let now = Utc::now();
    for url in urls.iter() {
        // let req = ur
        let product = scrape_product_price(url.clone(), agent.clone(), now)?;
        products.push(product);
    }
    Ok(products)
}

fn scrape_product_price(
    url: Url,
    client: Agent,
    time: DateTime<Utc>,
) -> Result<ProductInfo, Error> {
    let req = client.get(url.as_str());
    let req = set_headers(req);
    let res = req.call()?;
    let document = res.into_string()?;

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

pub fn get_product_name(url: &Url) -> Result<String, Error> {
    let client = ureq::agent();
    let req = client.get(url.as_str());
    let req = set_headers(req);
    let res = req.call()?;
    // dbg!(&res.charset());
    let document = res.into_string()?;
    // dbg!(&document);
    // println!("{}", &document);

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
