use futures::{stream, StreamExt};
use hyper::{client::Client, Body, Method, Request, StatusCode, Uri};
use rand::distributions::Alphanumeric;
use rand::{rngs::SmallRng, Rng, SeedableRng};
use std::time::{Instant, SystemTime};

type EResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const CONCURRENT_REQUESTS: usize = 1_000;

#[derive(Clone)]
struct CodeGenerator {
    rng: SmallRng,
}

impl CodeGenerator {
    fn new(rng: SmallRng) -> Self {
        Self { rng }
    }
}

impl Iterator for CodeGenerator {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        Some(
            std::iter::repeat(())
                .map(|()| self.rng.sample(Alphanumeric))
                .map(char::from)
                .take(17)
                .collect::<String>(),
        )
    }
}

async fn check_code<'a, C, S>(client: &hyper::Client<C>, code: S) -> EResult<Option<S>>
where
    C: 'static + hyper::client::connect::Connect + Send + Sync + Clone,
    S: AsRef<str>,
{
    let url = format!("https://discordapp.com/api/v9/entitlements/gift-codes/https://discord.gift/{code}?with_application=false&with_subscription_plan=true", code = code.as_ref()).parse::<Uri>()?;
    let response = client.get(url).await?;

    match response.status() {
        StatusCode::OK => Ok(Some(code)),
        _ => Ok(None),
    }
}

async fn webhook_send_code<'a, C, S>(
    client: &hyper::Client<C>,
    webhook_url: Uri,
    code: S,
) -> EResult<bool>
where
    C: 'static + hyper::client::connect::Connect + Send + Sync + Clone,
    S: AsRef<str>,
{
    let message = format!("Nitro code found: https://discord.gift/{}", code.as_ref());
    let request_body = format!("{{\"content\": \"{}\"}}", message);

    let request = Request::builder()
        .method(Method::POST)
        .uri(webhook_url)
        .header("content-type", "application/json")
        .body(Body::from(request_body))?;


    // @TODO: if webhook failed, return error message reported by discord
    let response = client.request(request).await?;
    match response.status() {
        StatusCode::OK => Ok(true),
        _ => Ok(false),
    }
}

#[tokio::main]
async fn main() -> EResult<()> {
    let connector = hyper_tls::HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(connector);
    let gen = CodeGenerator::new(SmallRng::from_entropy());

    let webhook_url = std::env::var("WEBHOOK_URL").expect("WEBHOOK_URL environment variable").parse::<Uri>()?;
    let check_amount = std::env::var("CHECK_AMOUNT").expect("CHECK_AMOUNT environment variable").parse::<usize>()?;

    let start_time = Instant::now();
    stream::iter(gen)
        .map(|code| check_code(&client, code))
        .buffer_unordered(CONCURRENT_REQUESTS)
        .take(check_amount)
        .for_each(|res| {
            let client = &client;
            let webhook_url = webhook_url.clone();
            let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("System time before UNIX EPOCH").as_millis();
            async move {
                match res {
                    Ok(Some(code)) => {
                        print!(
                            "[{}] Found valid code: https://discord.gift/{}. Took {:?}. ",
                            time,
                            code,
                            start_time.elapsed()
                        );
                        // send websocket request
                        match webhook_send_code(client, webhook_url, &code).await {
                            Ok(true) => eprintln!("Message to webhook successfully sent"),
                            Ok(false) => eprintln!("Failed to send message to webhook"),
                            Err(err) => eprintln!("Failed to send message to webhook: {}", err),
                        }
                        println!("");
                    }
                    Ok(None) => eprintln!("[{}] Checked code", time),
                    Err(err) => eprintln!("An error occured while checking a code: {}", err),
                }
            }
        })
        .await;

    println!("Checked {} codes in {:?}", check_amount, start_time.elapsed());

    Ok(())
}
