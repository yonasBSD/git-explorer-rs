use axum::{
    body::Body,
    extract::Path,
    extract::State,
    handler::HandlerWithoutStateExt,
    http::{header, StatusCode, Uri},
    response::Html,
    response::IntoResponse,
    response::Json,
    response::Redirect,
    routing::get,
    BoxError, Router,
};
use axum_extra::extract::Host;
use rust_embed::RustEmbed;
use axum_embed::ServeEmbed;
use axum_server::tls_rustls::RustlsConfig;
use clap::{App as ClapApp, Arg};
use rcgen::{CertificateParams, CertifiedKey, DistinguishedName, DnType, KeyPair};
use std::{fs, net::SocketAddr};
use tokio_util::io::ReaderStream;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(RustEmbed, Clone)]
#[folder = "assets/"]
struct Assets;

#[derive(Clone)]
struct AppState {
    repo_path: String,
}

#[derive(Debug, serde::Serialize)]
struct Commit {
    id: String,
    author: String,
    message: String,
    date: i64,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct Ports {
    http: u16,
    //https: u16,
}

async fn show_commit(
    Path((repo_name, commit_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Html<String> {
    let repo_path = state.repo_path.clone();

    // Open the Git repository
    let repo = match gix::discover(format!("{}/{}", repo_path, repo_name)) {
        Ok(repo) => repo,
        Err(_) => return Html(String::from("")),
    };

    // Lookup commit
    let commit = repo
        .find_commit(gix::ObjectId::from_hex(commit_id.as_bytes()).unwrap())
        .unwrap();

    // Build HTML response
    // Read HTML from a local file
    let html = match fs::read_to_string("templates/file.html") {
        Ok(html) => html,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            return Html(format!("Error reading file: {}", e));
        }
    };

    // Replace "{}" with the commit message
    let commit_message = String::from(commit.message().unwrap().title.to_string());
    let commit_name = commit.author().unwrap().name;
    let commit_email = commit.author().unwrap().email;

    Html(
        html.replace(
            "{}",
            format!(
                "{} &lt;{}&gt;<br/>{}",
                &commit_name, &commit_email, &commit_message
            )
            .as_str(),
        ),
    )
}

async fn get_commits_json(
    Path((repo_name,)): Path<(String,)>,
    State(state): State<AppState>,
) -> Json<Vec<Commit>> {
    let repo_path = state.repo_path.clone();

    // Open the Git repository
    let repo = match gix::discover(format!("{}/{}", repo_path, repo_name)) {
        Ok(repo) => repo,
        Err(_) => return Json(vec![]),
    };
    let commit = repo
        .rev_parse_single("HEAD")
        .unwrap()
        .object()
        .unwrap()
        .try_into_commit()
        .unwrap();
    let aa = repo
        .rev_walk([commit.id])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            Default::default(),
        ))
        .all()
        .unwrap();

    // Collect commits
    let mut commits: Vec<Commit> = vec![];
    for c in aa {
        let commit = c.unwrap().object().unwrap();

        let x = Some(Commit {
            id: commit.id.to_string(),
            author: String::from(format!(
                "{} [{}]",
                commit.author().unwrap().name,
                commit.author().unwrap().email
            )),
            message: String::from(commit.message().unwrap().title.to_string()),
            date: commit.time().unwrap().seconds,
        })
        .unwrap();
        commits.push(x);
    }

    Json(commits)
}

async fn get_commits(State(_state): State<AppState>) -> Html<String> {
    // Build HTML response
    // Read HTML from a local file
    let html = match fs::read_to_string("templates/commits.html") {
        Ok(html) => html,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            return Html(format!("Error reading file: {}", e));
        }
    };

    Html(html)
}

async fn get_static_file(
    Path((file_name,)): Path<(String,)>,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // `File` implements `AsyncRead`
    let file = match tokio::fs::File::open(file_name.clone()).await {
        Ok(file) => file,
        Err(err) => return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err))),
    };
    // convert the `AsyncRead` into a `Stream`
    let stream = ReaderStream::new(file);
    // convert the `Stream` into an `axum::body::HttpBody`
    let body = Body::from_stream(stream);

    let headers = match std::path::Path::new(&file_name)
        .extension()
        .unwrap()
        .to_str()
    {
        Some("html") =>  [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Some("css") =>   [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        Some("js") =>    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        Some("json") =>  [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        Some("png") =>   [(header::CONTENT_TYPE, "image/png; charset=utf-8")],
        Some("jpg") =>   [(header::CONTENT_TYPE, "image/jpeg; charset=utf-8")],
        Some("jpeg") =>  [(header::CONTENT_TYPE, "image/jpeg; charset=utf-8")],
        Some("gif") =>   [(header::CONTENT_TYPE, "image/gif; charset=utf-8")],
        Some("svg") =>   [(header::CONTENT_TYPE, "image/svg+xml; charset=utf-8")],
        Some("ico") =>   [(header::CONTENT_TYPE, "image/x-icon; charset=utf-8")],
        Some("ttf") =>   [(header::CONTENT_TYPE, "font/ttf; charset=utf-8")],
        Some("woff") =>  [(header::CONTENT_TYPE, "font/woff; charset=utf-8")],
        Some("woff2") => [(header::CONTENT_TYPE, "font/woff2; charset=utf-8")],
        Some("eot") =>   [(header::CONTENT_TYPE, "application/vnd.ms-fontobject; charset=utf-8")],
        Some("otf") =>   [(header::CONTENT_TYPE, "font/otf; charset=utf-8")],
        Some("txt") =>   [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        Some("pdf") =>   [(header::CONTENT_TYPE, "application/pdf; charset=utf-8")],
        Some("doc") =>   [(header::CONTENT_TYPE, "application/msword; charset=utf-8")],
        Some("docx") =>  [(header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.wordprocessingml.document; charset=utf-8")],
        Some("xls") =>   [(header::CONTENT_TYPE, "application/vnd.ms-excel; charset=utf-8")],
        Some("xlsx") =>  [(header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet; charset=utf-8")],
        Some("ppt") =>   [(header::CONTENT_TYPE, "application/vnd.ms-powerpoint; charset=utf-8")],
        Some("pptx") =>  [(header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.presentationml.presentation; charset=utf-8")],
        Some("xml") =>   [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        Some("zip") =>   [(header::CONTENT_TYPE, "application/zip; charset=utf-8")],
        Some("rar") =>   [(header::CONTENT_TYPE, "application/x-rar-compressed; charset=utf-8")],
        Some("7z") =>    [(header::CONTENT_TYPE, "application/x-7z-compressed; charset=utf-8")],
        Some("gz") =>    [(header::CONTENT_TYPE, "application/gzip; charset=utf-8")],
        Some("tar") =>   [(header::CONTENT_TYPE, "application/x-tar; charset=utf-8")],
        Some("swf") =>   [(header::CONTENT_TYPE, "application/x-shockwave-flash; charset=utf-8")],
        Some("flv") =>   [(header::CONTENT_TYPE, "video/x-flv; charset=utf-8")],
        Some("avi") =>   [(header::CONTENT_TYPE, "video/x-msvideo; charset=utf-8")],
        Some("mov") =>   [(header::CONTENT_TYPE, "video/quicktime; charset=utf-8")],
        Some("mp4") =>   [(header::CONTENT_TYPE, "video/mp4; charset=utf-8")],
        Some("mp3") =>   [(header::CONTENT_TYPE, "audio/mpeg; charset=utf-8")],
        Some("wav") =>   [(header::CONTENT_TYPE, "audio/x-wav; charset=utf-8")],
        Some("ogg") =>   [(header::CONTENT_TYPE, "audio/ogg; charset=utf-8")],
        Some("webm") =>  [(header::CONTENT_TYPE, "video/webm; charset=utf-8")],
        Some("mpg") =>   [(header::CONTENT_TYPE, "video/mpeg; charset=utf-8")],
        Some("mpeg") =>  [(header::CONTENT_TYPE, "video/mpeg; charset=utf-8")],
        Some("mpe") =>   [(header::CONTENT_TYPE, "video/mpeg; charset=utf-8")],
        Some("mp2") =>   [(header::CONTENT_TYPE, "video/mpeg; charset=utf-8")],
        Some("m4v") =>   [(header::CONTENT_TYPE, "video/x-m4v; charset=utf-8")],
        Some("3gp") =>   [(header::CONTENT_TYPE, "video/3gpp; charset=utf-8")],
        Some("3g2") =>   [(header::CONTENT_TYPE, "video/3gpp2; charset=utf-8")],
        Some("mkv") =>   [(header::CONTENT_TYPE, "video/x-matroska; charset=utf-8")],
        Some("amv") =>   [(header::CONTENT_TYPE, "video/x-matroska; charset=utf-8")],
        Some("m3u") =>   [(header::CONTENT_TYPE, "audio/x-mpegurl; charset=utf-8")],
        Some("m3u8") =>  [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl; charset=utf-8")],
        Some("ts") =>    [(header::CONTENT_TYPE, "video/mp2t; charset=utf-8")],
        Some("f4v") =>   [(header::CONTENT_TYPE, "video/mp4; charset=utf-8")],
        Some("f4p") =>   [(header::CONTENT_TYPE, "video/mp4; charset=utf-8")],
        Some("f4a") =>   [(header::CONTENT_TYPE, "video/mp4; charset=utf-8")],
        Some("f4b") =>   [(header::CONTENT_TYPE, "video/mp4; charset=utf-8")],
        Some("webp") =>  [(header::CONTENT_TYPE, "image/webp; charset=utf-8")],
        Some("bmp") =>   [(header::CONTENT_TYPE, "image/bmp; charset=utf-8")],
        Some("tif") =>   [(header::CONTENT_TYPE, "image/tiff; charset=utf-8")],
        Some("tiff") =>  [(header::CONTENT_TYPE, "image/tiff; charset=utf-8")],
        Some("psd") =>   [(header::CONTENT_TYPE, "image/vnd.adobe.photoshop; charset=utf-8")],
        Some("ai") =>    [(header::CONTENT_TYPE, "application/postscript; charset=utf-8")],
        Some("eps") =>   [(header::CONTENT_TYPE, "application/postscript; charset=utf-8")],
        Some("ps") =>    [(header::CONTENT_TYPE, "application/postscript; charset=utf-8")],
        Some("dwg") =>   [(header::CONTENT_TYPE, "image/vnd.dwg; charset=utf-8")],
        Some("dxf") =>   [(header::CONTENT_TYPE, "image/vnd.dxf; charset=utf-8")],
        Some("rtf") =>   [(header::CONTENT_TYPE, "application/rtf; charset=utf-8")],
        Some("odt") =>   [(header::CONTENT_TYPE, "application/vnd.oasis.opendocument.text; charset=utf-8")],
        Some("ods") =>   [(header::CONTENT_TYPE, "application/vnd.oasis.opendocument.spreadsheet; charset=utf-8")],
        Some("wasm") =>  [(header::CONTENT_TYPE, "application/wasm; charset=utf-8")],
        Some(&_) => [(header::CONTENT_TYPE, "application/octet-stream; charset=utf-8")],
        None => todo!(),
    };

    Ok((headers, body))
}

fn hostname() -> String {
    match hostname::get() {
        Ok(host) => String::from(host.to_str().unwrap()),
        Err(e) => {
            tracing::debug!("Error getting hostname: {}", e);
            String::from("localhost")
        }
    }
}

#[tokio::main]
async fn main() {
    // Parse command-line arguments
    let matches = ClapApp::new("Git Repository Viewer")
        .arg(
            Arg::with_name("repo_path")
                .short('r')
                .long("repo-path")
                .value_name("PATH")
                .help("Sets the path to the Git repositories directory")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("hostname")
                .short('n')
                .long("hostname")
                .value_name("HOSTNAME")
                .help("Sets the hostname for the certificate")
                .default_value(&hostname())
                .takes_value(true)
                .required(false),
        )
        .get_matches();

    let hostname = matches.value_of("hostname").unwrap();
    let repo_path = matches.value_of("repo_path").unwrap().to_string();
    let app_state = AppState { repo_path };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tls=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let ports = Ports {
        http: 8181,
        //https: 8182,
    };

    let serve_assets = ServeEmbed::<Assets>::new();

    // Create the router
    let app = Router::new()
        .route("/api/v1/repo/{repo_name}/commit/{commit_id}", get(show_commit))
        .route("/api/v1/repo/{repo_name}/commits/json", get(get_commits_json))
        .route("/api/v1/repo/{repo_name}/commits/all", get(get_commits))
        //.route("/static/{file_name}", get(get_static_file))
        .nest_service("/static", serve_assets)
        .with_state(app_state);


    // Define the server address
    let addr = SocketAddr::from(([127, 0, 0, 1], ports.http));

    // Start the https server
    println!("Starting web server on 0.0.0.0:{}", ports.http);
    tracing::debug!("listening on {}", addr);
    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
