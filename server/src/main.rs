//! A message relay server using OneSignal push services.
//!
//! 
//
// TODO: 
// * Proper error handling
// * Create `Client` separately and use a channel to send messages from server
// * Store server bind IP in config file

extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate toml;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate failure;
extern crate pretty_env_logger;
#[macro_use] extern crate log;

use std::fs::{OpenOptions, File, DirBuilder};
use std::str;
use std::io::{self, Write, Read};
use futures::future;
use hyper::service::service_fn;
use hyper::{header::HeaderValue, Client, Method, Request, Body,
    Response, Server, StatusCode, };
use hyper::rt::{Future, Stream};
use hyper_tls::HttpsConnector;


static ONESIGNAL_API_URI: &str = "https://onesignal.com/api/v1/notifications";


/// Configuration from toml.
#[derive(Debug, Serialize, Deserialize)]
struct Config {
    app_id: String,
    rest_api_key: String,
}

/// Incoming request message.
#[derive(Debug, Deserialize)]
struct Message {
    message: String,
}

/// OneSignal REST API outgoing request body message contents.
#[derive(Debug, Serialize)]
struct RequestBodyContents<'r> {
    en: &'r str,
}

/// OneSignal REST API outgoing request body.
#[derive(Debug, Serialize)]
struct RequestBody<'r> {
    app_id: &'r str,
    contents: RequestBodyContents<'r>,
    included_segments: &'r str,
}

/// Configuration errors.
#[derive(Fail)]
enum ConfigError {
    #[fail(display = "Unable to determine user home directory.")]
    NoHomeDir,
    #[fail(display = "Unable to convert path to a string.")]
    PathConversion,
    #[fail(display = "Error creating configuration directory: {}", _0)]
    CreateDir(io::Error),
    #[fail(display = "Error creating configuration file: {}", _0)]
    CreateFile(io::Error),
    #[fail(display = "Error reading configuration file: {}", _0)]
    ReadFile(io::Error),
}

impl std::fmt::Debug for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}



/// Parses or creates a configuration file at `$HOME/.cogciprocate`.
///
/// TODO:
/// * Error handling (call this function directly from main instead and handle errors there)
/// * Add keys and dummy values when file is first created as a convenience.
fn parse_config() -> Result<Config, ConfigError> {
    let mut config_string = String::new();
    let mut config_dir_path = ::std::env::home_dir().ok_or(ConfigError::NoHomeDir)?;
    config_dir_path.push(".cogciprocate/");

    let mut config_file_path = config_dir_path.clone();
    config_file_path.push("shoutit.toml");

    info!("Loading config from: '{}'", config_file_path.to_str().ok_or(ConfigError::PathConversion)?);

    let mut file = match File::open(&config_file_path) {
        Ok(f) => f,
        Err(_err) => {
            DirBuilder::new()
                .recursive(true)
                .create(config_dir_path.clone()).map_err(|err| ConfigError::CreateDir(err))?;
            OpenOptions::new().read(true).write(true).create(true).open(&config_file_path)
                .map_err(|err| ConfigError::CreateFile(err))?
            // TODO: Populate config file with placeholder values after creation.
        },        
    };

    file.read_to_string(&mut config_string).map_err(|err| ConfigError::ReadFile(err))?;

    let config = match toml::from_str(&config_string) {
        Ok(c) => c,
        Err(err) => panic!("Error parsing ShoutIt config file: {}. Please add 'app_id = YOUR_APP_ID' and \
            'rest_api_key = YOUR_REST_API_KEY' lines to '{}'.", err, config_file_path.to_str().unwrap()),
    };

    Ok(config)
}

lazy_static! {
    static ref CONFIG: Config = parse_config().unwrap();
}

type BoxFut = Box<Future<Item = Response<Body>, Error = hyper::Error> + Send>;


/// Responds to requests addressed to `/shout` by relaying the message within
/// request body to OneSignal for broadcast.
fn shout(req: Request<Body>) -> BoxFut {
    let mut response = Response::new(Body::empty());

    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => {
            *response.body_mut() = Body::from("Try POSTing data to /shout");
            Box::new(future::ok(response))

        },
        (&Method::POST, "/shout") => {
            *response.status_mut() = StatusCode::OK;
            let message_chunks = req.into_body().collect();

            let send = message_chunks.and_then(|message_chunks| {
                let message_bytes: Vec<u8> = message_chunks.into_iter().flat_map(|c| c.into_bytes()).collect();
                let message: Message = serde_json::from_slice(&message_bytes).unwrap();

                let body = RequestBody {
                    app_id: &CONFIG.app_id,
                    contents: RequestBodyContents {
                        en: &message.message,
                    },
                    included_segments: "[All]",
                };

                let uri: hyper::Uri = ONESIGNAL_API_URI.parse().unwrap();
                let mut req = Request::new(Body::from(serde_json::to_string(&body).unwrap()));
                *req.method_mut() = Method::POST;
                *req.uri_mut() = uri.clone();
                req.headers_mut().insert("content-type", HeaderValue::from_str("application/json").unwrap());
                req.headers_mut().insert("charset", HeaderValue::from_str("utf-8").unwrap());
                req.headers_mut().insert("Authorization", HeaderValue::from_str(
                    &format!("Basic {}", CONFIG.rest_api_key)).unwrap());

                // TODO: Create a custom service, `ShoutService`, containing a
                // tx to a separate struct containing a client and a rx. This
                // way the client does not need to be recreated for every
                // message.
                let https = HttpsConnector::new(4).expect("TLS initialization failed");
                let client = Client::builder()
                    .build::<_, hyper::Body>(https);

                info!("\nMessage: {}", message.message);

                client
                    .request(req)
                    .and_then(|res| {
                        info!("Response: {}", res.status());
                        // info!("Headers: {:#?}", res.headers());

                        res.into_body().for_each(|chunk| {
                            io::stdout().write_all(&chunk)
                                .map_err(|e| panic!("Example expects stdout is open, error: {}", e))
                        })
                    })
            });

            Box::new(send.join(future::ok(response)).map(|(_, r)| r))
        },
        _ => {
            *response.status_mut() = StatusCode::NOT_FOUND;
            Box::new(future::ok(response))
        },
    }
}


fn main() {
    pretty_env_logger::init();
    lazy_static::initialize(&CONFIG);

    info!("ShoutIt server starting...");

    let addr = ([10, 0, 0, 101], 8080).into();

    let server = Server::bind(&addr)
        .serve(|| service_fn(shout))
        .map_err(|e| error!("Server error: {}", e)); 

    info!("Listening on http://{}", addr);
    
    hyper::rt::run(server);

    info!("ShoutIt server shut down.");
}


