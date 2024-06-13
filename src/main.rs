use axum::{
    response::IntoResponse,
    response::Html,
    response::Json,
    response::Redirect,
    extract::Path,
    extract::State,
    extract::Host,
    routing::get,
    http::{header, StatusCode, Uri},
    body::Body,
    handler::HandlerWithoutStateExt,
    BoxError,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use tokio_util::io::ReaderStream;

/*
use rustls::internal::pemfile::{certs, rsa_private_keys};
use rustls::{server::NoClientAuth, ServerConfig};
use rustls::crypto::aws_lc_rs::sign::any_ecdsa_type;
use rustls::HandshakeType::Certificate;
*/
use rcgen::{generate_simple_self_signed, CertifiedKey};
use std::{net::SocketAddr, fs};
use git2::{Repository, Oid};
use clap::{App as ClapApp, Arg};

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
    https: u16,
}

async fn show_commit(
    Path((repo_name, commit_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Html<String> {
    let repo_path = state.repo_path.clone();

    // Open the Git repository
    let repo_path2 = format!("{}/{}", repo_path, repo_name);
    let repo = match Repository::open(&repo_path2) {
        Ok(repo) => repo,
        Err(_) => return Html("Repository not found".into()),
    };

    // Resolve commit ID to OID
    let commit_oid = match Oid::from_str(&commit_id) {
        Ok(oid) => oid,
        Err(_) => return Html("Invalid commit ID".into()),
    };

    // Lookup commit by OID
    let commit = match repo.find_commit(commit_oid) {
        Ok(commit) => commit,
        Err(_) => return Html("Commit not found".into()),
    };

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
    let commit_message = commit.message().unwrap_or("No message").replace("\n", "<br/>");
    let commit_author = commit.author();
    let commit_name = commit_author.name().unwrap_or("");
    let commit_email = commit_author.email().unwrap_or("");

    Html(html.replace("{}", format!("{} &lt;{}&gt;<br/>{}", &commit_name, &commit_email, &commit_message).as_str()))
}

async fn get_commits_json(
    Path((repo_name,)): Path<(String,)>, 
    State(state): State<AppState>
) -> Json<Vec<Commit>>
{
    let repo_path = state.repo_path.clone();

    // Open the Git repository
    let repo_path2 = format!("{}/{}", repo_path, repo_name);
    let repo = match Repository::open(&repo_path2) {
        Ok(repo) => repo,
        Err(_) => return Json(vec![]),
    };

    let mut revwalk = match repo.revwalk() {
        Ok(revwalk) => revwalk,
        Err(err) => {
            eprintln!("Failed to create revwalk: {}", err);
            return Json(vec![]);
        }
    };

    // Set the sorting order
    revwalk.set_sorting(git2::Sort::TIME).unwrap();

    // Start from the HEAD of the master branch
    revwalk.push_head().unwrap();

    // Collect commits
    let commits: Vec<Commit> = revwalk
        .filter_map(|oid| {
            let oid = match oid {
                Ok(oid) => oid,
                Err(_) => return None,
            };

            let commit = match repo.find_commit(oid) {
                Ok(commit) => commit,
                Err(_) => return None,
            };

            let x = Some(Commit {
                id: oid.to_string(),
                author: String::from(format!("{} [{}]", commit.author().name().unwrap(), commit.author().email().unwrap_or(""))),
                message: String::from(commit.message().unwrap()),
                date: commit.time().seconds(),
            }); x
        })
        .collect();

    Json(commits)
}

async fn get_commits(State(_state): State<AppState>) -> Html<String>
{
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
    State(_state): State<AppState>
) -> impl IntoResponse
{
    // `File` implements `AsyncRead`
    let file = match tokio::fs::File::open(file_name.clone()).await {
        Ok(file) => file,
        Err(err) => return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err))),
    };
    // convert the `AsyncRead` into a `Stream`
    let stream = ReaderStream::new(file);
    // convert the `Stream` into an `axum::body::HttpBody`
    let body = Body::from_stream(stream);

    let headers = match std::path::Path::new(&file_name).extension().unwrap().to_str() {
        Some("js") => [ (header::CONTENT_TYPE, "text/javascript; charset=utf-8"), ],
        None => todo!(),
        Some(&_) => todo!()
    };

    Ok((headers, body))
}

#[allow(dead_code)]
async fn redirect_http_to_https(ports: Ports) {
    fn make_https(host: String, uri: Uri, ports: Ports) -> Result<Uri, BoxError> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(axum::http::uri::Scheme::HTTPS);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        let https_host = host.replace(&ports.http.to_string(), &ports.https.to_string());
        parts.authority = Some(https_host.parse()?);

        Ok(Uri::from_parts(parts)?)
    }

    let redirect = move |Host(host): Host, uri: Uri| async move {
        match make_https(host, uri, ports) {
            Ok(uri) => Ok(Redirect::permanent(&uri.to_string())),
            Err(error) => {
                tracing::warn!(%error, "failed to convert URI to HTTPS");
                Err(StatusCode::BAD_REQUEST)
            }
        }
    };

    let addr = SocketAddr::from(([127, 0, 0, 1], ports.http));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, redirect.into_make_service())
        .await
        .unwrap();
}

fn generate_self_signed_cert(hostname: &str) -> Result<CertifiedKey, rcgen::Error> {
/*
    // Generate ECC private key
    let rsa_key = Arc::new(any_ecdsa_type(&rustls::generate_ecdsa_key)).into();
    let mut config = ServerConfig::new(NoClientAuth::new());
    config.set_single_cert(vec![Certificate(rsa_key.clone())], rustls::PrivateKey(rsa_key.clone()));

    // Write private key and certificate to disk
    let mut cert_file = File::create("cert.pem")?;
    let mut key_file = File::create("key.pem")?;

    let mut key_buf = vec![];
    config.key.write_pem(&mut key_buf).unwrap();
    key_file.write_all(&key_buf)?;

    let mut cert_buf = vec![];
    let common_name = format!("CN={}", hostname);
    config.certificates[0].write_pem(&mut cert_buf).unwrap();
    cert_file.write_all(&cert_buf)?;
    Ok(())
*/
    let subject_alt_names = vec![hostname.to_string()];
    let cert = generate_simple_self_signed(subject_alt_names).unwrap();

    // The certificate is now valid for hostname
    println!("{}", cert.key_pair.serialize_pem());
    println!("{}", cert.cert.pem());

    Ok(cert)
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
                .required(true))
        .arg(
            Arg::with_name("hostname")
                .short('n')
                .long("hostname")
                .value_name("HOSTNAME")
                .help("Sets the hostname for the certificate")
                .default_value("localhost")
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
        http: 8080,
        https: 8081,
    };

    // optional: spawn a second server to redirect http requests to this server
    //tokio::spawn(redirect_http_to_https(ports));

    // Load your SSL certificate and private key
    let cert = match generate_self_signed_cert(hostname) {
        Ok(cert) => cert,
        Err(e) => {
            println!("Erorr: unable to generate self-signed certificates for HTTPS");
            println!("{}", e);
            std::process::exit(1);
        }
    };

    let config = RustlsConfig::from_pem(cert.cert.pem().into(), cert.key_pair.serialize_pem().into())
        .await
        .unwrap();

    // Create the router
    let app = Router::new()
        .route(
            "/repo/:repo_name/commit/:commit_id",
            get(show_commit)
        )
        .route(
            "/repo/:repo_name/commits/json",
            get(get_commits_json)
        )
        .route(
            "/repo/:repo_name/commits/all",
            get(get_commits)
        )
        .route(
            "/static/:file_name",
            get(get_static_file)
        )
        .with_state(app_state);

    // Define the server address
    let addr = SocketAddr::from(([0, 0, 0, 0], ports.https));

    // Start the https server
    println!("Starting web server on 0.0.0.0:{}", ports.https);
    tracing::debug!("listening on {}", addr);
    axum_server::bind_rustls(addr, config)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
