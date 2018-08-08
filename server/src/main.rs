//! A message relay server using OneSignal push services.
//!
//! 
//
// * Store server bind IP in config file

extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate tokio_core;
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
use std::thread;
use std::net::SocketAddr;
use failure::{Context};
use futures::future;
use hyper::{header::HeaderValue, Client, Method, Request, Body,
    Response, StatusCode, service::Service, server::conn::Http};
use hyper::rt::{Future, Stream};
use hyper_tls::HttpsConnector;
use tokio_core::{reactor::Core, net::TcpListener};


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

/// Parses or creates the configuration file, `$HOME/.cogciprocate/shoutit.toml`.
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
        // TODO: Populate config file with placeholder values after creation.
        Err(err) => panic!("Error parsing ShoutIt config file: {}. Please add 'app_id = YOUR_APP_ID' and \
            'rest_api_key = YOUR_REST_API_KEY' lines to '{}'.", err, config_file_path.to_str().unwrap()),
    };

    // Ensure configuration file values are valid:
    HeaderValue::from_str(&config.app_id).map_err(|err| ConfigError::InvalidAppId(err))?;
    HeaderValue::from_str(&config.rest_api_key).map_err(|err| ConfigError::InvalidRestApiKey(err))?;

    Ok(config)
}

lazy_static! {
    // Print error with `Display` rather than `Debug` format:
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
//
// This is unweildy because of the stringent requirements on error types. 
//
// Using a `NewService` with `Http::serve_addr` or `Http::serve_addr_handle` 
// may alleviate this (TODO).
#[derive(Debug, Fail)]
enum ServerErrorKind {
    #[fail(display = "{}", _0)]
    Hyper(hyper::Error),
    #[fail(display = "{}", _0)]
    SerdeJson(serde_json::Error),
    #[fail(display = "Failed to bind server to address '{}': {}", _0, _1)]
    BindAddress(SocketAddr, io::Error),
    #[fail(display = "The server has shut down unexpectedly")]
    UnexpectedEnd,
    #[fail(display = "Thread spawn error: {}", _0)]
    ThreadSpawnError(io::Error),
}

#[derive(Debug)]
struct ServerError {
    inner: Context<ServerErrorKind>,
}

impl ServerError {
    fn new(kind: ServerErrorKind) -> ServerError {
        ServerError { inner: Context::new(kind) }
    }

    fn bind_address(addr: SocketAddr, err: io::Error) -> ServerError {
        ServerError::new(ServerErrorKind::BindAddress(addr, err))
    }

    fn unexpected_end() -> ServerError {
        ServerError::new(ServerErrorKind::UnexpectedEnd)
    }

    fn thread_spawn_error(err: io::Error) -> ServerError {
        ServerError::new(ServerErrorKind::ThreadSpawnError(err))
    }
}

impl std::error::Error for ServerError {    
    fn description(&self) -> &str {
        match self.inner.get_context() {
            ServerErrorKind::Hyper(err) => err.description(),
            ServerErrorKind::SerdeJson(err) => err.description(),
            ServerErrorKind::BindAddress(_, err) => err.description(),
            ServerErrorKind::UnexpectedEnd => "The server has shut down unexpectedly",
            ServerErrorKind::ThreadSpawnError(err) => err.description(),
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
        ServerError::new(ServerErrorKind::Hyper(err))
    }
}

impl From<serde_json::Error> for ServerError {
    fn from(err: serde_json::Error) -> ServerError {
        ServerError::new(ServerErrorKind::SerdeJson(err))
    }
}

unsafe impl Send for ServerError {}
unsafe impl Sync for ServerError {}


/// The ShoutIt server.
#[derive(Clone, Debug)]
struct ShoutServer {
    client: Client<HttpsConnector<hyper::client::HttpConnector>, Body>,
}

impl ShoutServer {
    /// Returns a new `ShoutServer`.
    fn new() -> ShoutServer {
        let https = HttpsConnector::new(4).expect("TLS initialization failed");
        let client = Client::builder().build(https);
        ShoutServer { client }
    }

    /// Starts the server in a new thread and returns the thread handle.
    fn run(self) -> Result<thread::JoinHandle<Result<(), ServerError>>, ServerError> {
        thread::Builder::new()
            .name("shout_server".to_owned())
            .spawn(move || -> Result<(), ServerError> {  
                let address = ([10, 0, 0, 101], 8080).into();
                // let address = ([127, 0, 0, 1], 8080).into();
                let mut core = Core::new().unwrap();
                let handle = core.handle();
    
                let http = Http::new();

                let listener = TcpListener::bind(&address, &handle)
                    .map_err(|err| ServerError::bind_address(address.clone(), err))?;

                let server = listener.incoming().for_each(move |(socket, _source_address)| {
                    handle.spawn(
                        http.serve_connection(socket, self.clone())
                            .map(|_| ())
                            .map_err(|err| error!("Server connection error: {}", err))
                    );
                    Ok(())
                });

                info!("Listening on http://{}", address);
                
                core.run(server).unwrap();
                Err(ServerError::unexpected_end())
            }).map_err(|err| ServerError::thread_spawn_error(err))
    }    
}

impl Service for ShoutServer {
    type ReqBody = Body;
    type ResBody = Body;
    type Error = ServerError;
    type Future = Box<Future<Item = Response<Self::ResBody>, Error = Self::Error> + Send>;

    fn call(&mut self, req: Request<Self::ReqBody>) -> Self::Future {
        let mut response_out = Response::new(Body::empty());
        let client = self.client.clone();

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
                }).and_then(move |(message, body)| {
                    let uri: hyper::Uri = ONESIGNAL_API_URI.parse().unwrap();
                    let mut req = Request::new(body);
                    *req.method_mut() = Method::POST;
                    *req.uri_mut() = uri.clone();
                    req.headers_mut().insert("content-type", HeaderValue::from_str("application/json").unwrap());
                    req.headers_mut().insert("charset", HeaderValue::from_str("utf-8").unwrap());
                    req.headers_mut().insert("Authorization", HeaderValue::from_str(
                        &format!("Basic {}", CONFIG.rest_api_key)).unwrap());

                    info!("Message: {}", message.message);

                    client.request(req).and_then(move |response_in| {
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
}


fn main() {
    pretty_env_logger::init();
    lazy_static::initialize(&CONFIG);

    info!("ShoutIt server starting...");

    let result = ShoutServer::new().run().and_then(|handle| {
        handle.join().unwrap()
    });

    match result {
        Ok(()) => (),
        Err(err) => panic!("{}", err),
    }

    info!("ShoutIt server shut down.");
}
