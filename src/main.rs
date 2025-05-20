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
use axum_server::tls_rustls::RustlsConfig;
use clap::{App as ClapApp, Arg};
use rcgen::{CertificateParams, CertifiedKey, DistinguishedName, DnType, KeyPair};
use std::{fs, net::SocketAddr};
use tokio_util::io::ReaderStream;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
    let html = match fs::read_to_string("file.html") {
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
    let html = match fs::read_to_string("commits.html") {
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
        Some("js") => [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        None => todo!(),
        Some(&_) => todo!(),
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

    // Create the router
    let app = Router::new()
        .route("/api/v1/repo/{repo_name}/commit/{commit_id}", get(show_commit))
        .route("/api/v1/repo/{repo_name}/commits/json", get(get_commits_json))
        .route("/api/v1/repo/{repo_name}/commits/all", get(get_commits))
        .route("/static/{file_name}", get(get_static_file))
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
