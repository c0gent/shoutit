//! A message relay server using OneSignal push services.

#![allow(unused_imports, dead_code)]

extern crate futures;
extern crate tokio;
extern crate hyper;
extern crate hyper_tls;
extern crate toml;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;
extern crate serde_json;

use std::fs::{OpenOptions, File, DirBuilder};
use std::str;
use std::io::{self, Write, Read, Error as IoError};
use futures::{future, IntoFuture};
use tokio::reactor::Reactor;
use hyper::service::service_fn;
use hyper::{error::Error as HyperError, header::HeaderValue, Client, Method, Request, Body,
    Response, Server, StatusCode, service::Service, body::Payload};
use hyper::rt::{self, Future, Stream};
use hyper_tls::HttpsConnector;
use toml::Value;


#[derive(Debug, Deserialize)]
struct Config {
    app_id: String,
    rest_api_key: String,
}

#[derive(Debug, Deserialize)]
struct Message {
    message: String,
}


fn parse_config() -> Config {
    let mut config_string = String::new();
    let mut config_dir_path = ::std::env::home_dir().expect("Unable to determine user home directory.");
    config_dir_path.push(".cogciprocate/");

    let mut config_file_path = config_dir_path.clone();
    config_file_path.push("shoutit.toml");

    println!("{}", config_file_path.to_str().unwrap());

    let mut file = match File::open(&config_file_path) {
        Ok(f) => f,
        Err(err) => {
            DirBuilder::new()
                .recursive(true)
                .create(config_dir_path.clone()).unwrap();
            let _ = OpenOptions::new().write(true).create(true).open(&config_file_path).unwrap();
            File::open(&config_file_path).expect(&format!("Error opening ShoutIt config file: {}", err))
        },        
    };

    file.read_to_string(&mut config_string).unwrap();

    let config = match toml::from_str(&config_string) {
        Ok(c) => c,
        Err(err) => panic!("Error parsing ShoutIt config file: {}. Please add 'app_id = YOUR_APP_ID' and \
            'rest_api_key = YOUR_REST_API_KEY' lines to '{}'.", err, config_file_path.to_str().unwrap()),
    };

    config
}

lazy_static! {
    static ref CONFIG: Config = parse_config();
}

type BoxFut = Box<Future<Item = Response<Body>, Error = hyper::Error> + Send>;


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
                // let message = String::from_utf8(message_bytes).unwrap();

                let message: Message = serde_json::from_slice(&message_bytes).unwrap();

                let json = format!("{{\"app_id\": \"{}\", \
                    \"contents\": {{\"en\": \"{}\"}}, \
                    \"included_segments\": [\"All\"]}}", CONFIG.app_id, message.message);

                let uri: hyper::Uri = "https://onesignal.com/api/v1/notifications".parse().unwrap();
                let mut req = Request::new(Body::from(json));
                *req.method_mut() = Method::POST;
                *req.uri_mut() = uri.clone();
                req.headers_mut().insert("content-type", HeaderValue::from_str("application/json").unwrap());
                req.headers_mut().insert("charset", HeaderValue::from_str("utf-8").unwrap());
                req.headers_mut().insert("Authorization", HeaderValue::from_str(&format!("Basic {}", CONFIG.rest_api_key)).unwrap());

                let https = HttpsConnector::new(4).expect("TLS initialization failed");
                let client = Client::builder()
                    .build::<_, hyper::Body>(https);

                println!("\nMessage: {}", message.message);

                client
                    .request(req)
                    .and_then(|res| {
                        println!("Response: {}", res.status());
                        // println!("Headers: {:#?}", res.headers());

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
    println!("ShoutIt server started.");

    let addr = ([10, 0, 0, 101], 8080).into();

    let server = Server::bind(&addr)
        .serve(|| service_fn(shout))
        .map_err(|e| eprintln!("Server error: {}", e)); 

    println!("Listening on http://{}", addr);
    
    hyper::rt::run(server);

    println!("ShoutIt server shutting down.");
}


