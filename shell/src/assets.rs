use std::borrow::Cow;

use http::{Request, Response, StatusCode, header};
use include_dir::{Dir, include_dir};

static UI_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../ui/dist");

pub fn serve(request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let requested_path = request.uri().path().trim_start_matches('/');
    let asset_path = if requested_path.is_empty() {
        "index.html"
    } else {
        requested_path
    };
    let file = UI_ASSETS.get_file(asset_path).or_else(|| {
        (!asset_path.contains('.'))
            .then(|| UI_ASSETS.get_file("index.html"))
            .flatten()
    });

    match file {
        Some(file) => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                mime_guess::from_path(asset_path)
                    .first_or_octet_stream()
                    .as_ref(),
            )
            .header(header::CACHE_CONTROL, "no-store")
            .body(Cow::Borrowed(file.contents()))
            .expect("valid asset response"),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Cow::Borrowed(&b"Not found"[..]))
            .expect("valid not-found response"),
    }
}
