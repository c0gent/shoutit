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
use std::io::{self, Read};
use failure::{Context};
use futures::future;
use hyper::service::service_fn;
use hyper::{header::HeaderValue, Client, Method, Request, Body,
    Response, Server, StatusCode, };
use hyper::rt::{Future, Stream};
use hyper_tls::HttpsConnector;


static ONESIGNAL_API_URI: &str = "https://onesignal.com/api/v1/notifications";
static INCLUDED_SEGMENTS: &[&str] = &["All"];


/// Configuration from toml.
#[derive(Debug, Serialize, Deserialize)]
struct Config {
    app_id: String,
    rest_api_key: String,
}

/// Configuration errors.
#[derive(Debug, Fail)]
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
    #[fail(display = "Invalid App ID in configuration file: {}", _0)]
    InvalidAppId(hyper::header::InvalidHeaderValue),
    #[fail(display = "Invalid REST API key in configuration file: {}", _0)]
    InvalidRestApiKey(hyper::header::InvalidHeaderValue),
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

    let config = match toml::from_str::<Config>(&config_string) {
        Ok(c) => c,
        Err(err) => panic!("Error parsing ShoutIt config file: {}. Please add 'app_id = YOUR_APP_ID' and \
            'rest_api_key = YOUR_REST_API_KEY' lines to '{}'.", err, config_file_path.to_str().unwrap()),
    };

    // Ensure configuration file values are valid:
    HeaderValue::from_str(&config.app_id).map_err(|err| ConfigError::InvalidAppId(err))?;
    HeaderValue::from_str(&config.rest_api_key).map_err(|err| ConfigError::InvalidRestApiKey(err))?;

    Ok(config)
}


lazy_static! {
    // Print with `Display` rather than `Debug` format:
    static ref CONFIG: Config = parse_config().unwrap_or_else(|err| panic!("{}", err));
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
    included_segments: &'r [&'r str],
}

/// Server errors.
///
/// This is unweildy because of the weirdly stringent requirements on error 
/// types that non-custom servers have (when using `hyper::server::Builder::build`). 
/// This will probably be alleviated when futures 0.3 is integrated into hyper and 
/// error handling changes.
///
/// Using a custom server would also eliminate the problem (TODO). 
#[derive(Debug, Fail)]
enum ServerErrorKind {
    #[fail(display = "{}", _0)]
    Hyper(hyper::Error),
    #[fail(display = "{}", _0)]
    SerdeJson(serde_json::Error),
}

#[derive(Debug)]
struct ServerError {
    inner: Context<ServerErrorKind>,
}

impl std::error::Error for ServerError {    
    fn description(&self) -> &str {
        match self.inner.get_context() {
            ServerErrorKind::Hyper(err) => err.description(),
            ServerErrorKind::SerdeJson(err) => err.description(),
        }
    }

    fn cause(&self) -> Option<&std::error::Error> {
        Some(&*self as &std::error::Error)
    }
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.inner, f)
    }
}

impl From<hyper::Error> for ServerError {
    fn from(err: hyper::Error) -> ServerError {
        ServerError { inner: Context::new(ServerErrorKind::Hyper(err)) }
    }
}

impl From<serde_json::Error> for ServerError {
    fn from(err: serde_json::Error) -> ServerError {
        ServerError { inner: Context::new(ServerErrorKind::SerdeJson(err)) }
    }
}

unsafe impl Send for ServerError {}
unsafe impl Sync for ServerError {}


type BoxFut = Box<Future<Item = Response<Body>, Error = ServerError> + Send>;


/// Responds to requests addressed to `/shout` by relaying the message within
/// request body to OneSignal for broadcast.
fn shout(req: Request<Body>) -> BoxFut {
    let mut response_out = Response::new(Body::empty());

    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => {
            *response_out.body_mut() = Body::from("Try POSTing data to /shout");
            Box::new(future::ok(response_out))

        },
        (&Method::POST, "/shout") => {
            *response_out.status_mut() = StatusCode::OK;
            let message_chunks = req.into_body().collect().map_err(|err| ServerError::from(err));

            let actions = message_chunks.and_then(|message_chunks| {
                let message_bytes: Vec<u8> = message_chunks.into_iter().flat_map(|c| c.into_bytes()).collect();
                match serde_json::from_slice::<Message>(&message_bytes) {
                    Ok(b) => future::ok(b),
                    Err(err) => future::err(ServerError::from(err)),
                }
            }).and_then(|message| {
                let b = {
                    let rb = RequestBody {
                        app_id: &CONFIG.app_id,
                        contents: RequestBodyContents {
                            en: &message.message,
                        },
                        included_segments: INCLUDED_SEGMENTS,
                    };
                    match serde_json::to_string(&rb) {
                        Ok(rb_str) => Body::from(rb_str),
                        Err(err) => return future::err(ServerError::from(err)),
                    }
                };
                future::ok((message, b))
            }).and_then(|(message, body)| {
                let uri: hyper::Uri = ONESIGNAL_API_URI.parse().unwrap();
                let mut req = Request::new(body);
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

                info!("Message: {}", message.message);

                client
                    .request(req)
                    .and_then(move |response_in| {
                        info!("Response: {}", response_in.status());
                        trace!("Headers: {:#?}", response_in.headers());

                        response_in.into_body().concat2().and_then(|body_in| {
                            let body_bytes = body_in.into_bytes();
                            info!("Incoming response: {}",  String::from_utf8_lossy(&body_bytes));
                            *response_out.body_mut() = Body::from(body_bytes);
                            future::ok(response_out)
                        })
                    })
                    .map_err(|err| ServerError::from(err))
            });

            Box::new(actions)
        },
        _ => {
            *response_out.status_mut() = StatusCode::NOT_FOUND;
            Box::new(future::ok(response_out))
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


