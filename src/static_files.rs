//! バイナリに埋め込んだ静的ファイル (TORO のアイコン)

use axum::http::header::{self, HeaderName, HeaderValue};
use axum::response::Response;

use crate::http;

const IMAGE_CACHE: &str = "public, max-age=86400";

pub fn serve(path: &str) -> Option<Response> {
    // AI クライアントや検索エンジンは、決まった場所のアイコンを取りに来る
    let bytes: &'static [u8] = match path {
        "/favicon.ico" | "/favicon.png" | "/_assets/favicon-32.png" => {
            include_bytes!("../assets/icons/favicon-32.png")
        }
        "/apple-touch-icon.png" => include_bytes!("../assets/icons/apple-touch-icon.png"),
        "/icon-64.png" => include_bytes!("../assets/icons/icon-64.png"),
        "/icon-192.png" => include_bytes!("../assets/icons/icon-192.png"),
        "/icon-512.png" => include_bytes!("../assets/icons/icon-512.png"),
        _ => return None,
    };
    let mut headers = http::public_headers();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(IMAGE_CACHE));
    // AI クライアントの画面が、別のオリジンからアイコンを読めるように
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("cross-origin"),
    );
    Some(http::respond(200, headers, "image/png", bytes))
}
