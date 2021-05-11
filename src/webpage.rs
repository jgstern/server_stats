use std::borrow::Cow;

use actix_web::{body::Body, HttpRequest, HttpResponse, Result};
use askama_actix::{Template, TemplateToResponse};
use mime_guess::from_path;
use rust_embed::RustEmbed;

pub mod sse;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate;

#[derive(Template)]
#[template(path = "2d.html")]
struct TwoDTemplate;

#[derive(Template)]
#[template(path = "vr.html")]
struct VRTemplate;
#[derive(Template)]
#[template(path = "ar.html")]
struct ARTemplate;

pub async fn index_page() -> Result<HttpResponse> {
    IndexTemplate {}.to_response()
}

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

fn handle_embedded_file(path: &str) -> HttpResponse {
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
        None => HttpResponse::NotFound().body("404 Not Found"),
    }
}

pub async fn assets(req: HttpRequest) -> HttpResponse {
    let path: &str = req.match_info().query("filename");
    handle_embedded_file(path)
}
