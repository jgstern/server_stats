use std::borrow::Cow;

use actix_web::{body::Body, HttpRequest, HttpResponse, Result};
use askama_actix::{Template, TemplateToResponse};
use mime_guess::from_path;
use rust_embed::RustEmbed;

pub mod ws;

#[derive(Template)]
#[template(path = "2d.html")]
struct TwoDTemplate;

#[derive(Template)]
#[template(path = "vr.html")]
struct VRTemplate;
#[derive(Template)]
#[template(path = "ar.html")]
struct ARTemplate;

pub async fn two_d_page() -> Result<HttpResponse> {
    TwoDTemplate {}.to_response()
}

pub async fn vr_page() -> Result<HttpResponse> {
    VRTemplate {}.to_response()
}

pub async fn ar_page() -> Result<HttpResponse> {
    ARTemplate {}.to_response()
}

#[derive(RustEmbed)]
#[folder = "assets"]
struct Asset;

#[derive(RustEmbed)]
#[folder = "webpage/dist/server-stats"]
struct Webpage;

fn handle_embedded_assets_file(path: &str) -> HttpResponse {
    match Asset::get(path) {
        Some(content) => {
            let body: Body = match content {
                Cow::Borrowed(bytes) => bytes.into(),
                Cow::Owned(bytes) => bytes.into(),
            };
            HttpResponse::Ok()
                .content_type(from_path(path).first_or_octet_stream().as_ref())
                .body(body)
        }
        None => handle_embedded_file(&path.replace("/assets", "")),
    }
}

pub async fn index_page() -> HttpResponse {
    handle_embedded_file("index.html")
}

fn handle_embedded_file(path: &str) -> HttpResponse {
    match Webpage::get(path) {
        Some(content) => {
            let body: Body = match content {
                Cow::Borrowed(bytes) => bytes.into(),
                Cow::Owned(bytes) => bytes.into(),
            };
            HttpResponse::Ok()
                .content_type(from_path(path).first_or_octet_stream().as_ref())
                .body(body)
        }
        None => HttpResponse::NotFound().body("404 Not Found"),
    }
}

pub async fn assets(req: HttpRequest) -> HttpResponse {
    let path: &str = req.match_info().query("filename");
    handle_embedded_assets_file(path)
}

pub async fn webpage(req: HttpRequest) -> HttpResponse {
    let path: &str = req.match_info().query("filename");
    if path == "metrics" {
        return HttpResponse::Ok().finish();
    }
    handle_embedded_file(path)
}
