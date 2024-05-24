use std::collections::HashMap;

use anyhow::bail;
use serde::{Deserialize, Serialize, de::Visitor};
use tracing::{debug, info};

#[derive(clap::Args, Debug, Clone)]
#[command()]
pub struct Arguments {
    #[command(subcommand)]
    action: Actions,
    /// use this file for api list
    /// if this is not specified then <service name>.json will be used as api file
    #[arg(short = 'f', long)]
    api_file: Option<std::path::PathBuf>,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum Actions {
    /// execute a rest api call
    #[command()]
    Exec{
        #[arg()]
        api_name: String,
    },
    /// Create a new api
    #[command()]
    Insert,
    /// view given api details
    #[command()]
    View,
    /// list all the available api's
    #[command()]
    List,
}

/// target url
#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum Uri {
    /// complete url in one string
    TargetString(String),
    /// refer https://docs.rs/http/latest/http/uri/struct.Uri.html
    Pieces {
        scheme: String,
        authority: String,
        path: String,
        query: Option<HashMap<String, String>>,
        fragment: Option<String>,
    },
}

#[derive(Debug, PartialEq)]
struct HttpMethods(reqwest::Method);

impl Serialize for HttpMethods{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
            serializer.serialize_str(self.0.to_string().as_str())
    }
}

struct HttpMethodsVisitor;

impl<'de> Visitor<'de> for HttpMethodsVisitor {
    type Value=HttpMethods;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("expecting string reprentation of reqwest::Method")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let method:reqwest::Method = reqwest::Method::from_bytes(v.as_bytes())
            .or_else(|_|Err(serde::de::Error::custom("Unknown Http Method")))?;
        Ok(HttpMethods(method))
    }

}

impl<'de> Deserialize<'de> for HttpMethods {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
            deserializer.deserialize_str(HttpMethodsVisitor)
    }
}


#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Api {
    uri: Uri,
    method: HttpMethods,
    headers: Vec<(String, String)>,
    data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct ApiFile {
    apis: HashMap<String, Api>,
}

pub async fn handler(args: &Arguments, prefix_dir: &std::path::PathBuf) -> Result<(), anyhow::Error> {
    debug!("args {args:?}");
    let mut api_file_path = prefix_dir.clone();
    api_file_path.push(&args.api_file);
    debug!("api file: {api_file_path:?}");
    match &args.action {
        Actions::Exec{api_name} => execute(&api_file_path, api_name).await,
        Actions::Insert => todo!(),
        Actions::View => todo!(),
        Actions::List => todo!(),
    }
}

async fn execute(api_file_path: &impl AsRef<std::path::Path>, api_name: &str) -> Result<(), anyhow::Error> {
    let content = std::fs::read_to_string(api_file_path)?;
    let api_file_cont: ApiFile = serde_json::from_str(&content)?;
    let Some(api) = api_file_cont.apis.get(api_name) else {
        bail!("Failed find api {api_name} in api collection")
    };
    let url = match &api.uri {
        Uri::TargetString(url_str) => {
            reqwest::Url::parse(url_str)?
        },
        Uri::Pieces { scheme, authority, path, query, fragment } => todo!(),
    };
    let client = reqwest::Client::new();
    let req = client.request(api.method.0.clone(), url)
        .build()?;
    info!("Executing Request: {req:?}");
    let resp = client.execute(req).await?;
    info!("Recieved Response: {resp:?}");
    if resp.status().is_success(){
        println!("{}", resp.text().await?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn uri_parse() {
        assert_eq!(Uri::Pieces {
               scheme: "https".to_string(),
               authority: "127.0.0.1".to_string(),
               path: "abc/def".to_string(),
               query: None,
               fragment: None 
           },
           serde_json::from_str(r#"{"Pieces":{"scheme":"https","authority":"127.0.0.1","path":"abc/def","query":null,"fragment":null}}"#).unwrap()
        );
        assert_eq!(
            Uri::TargetString("https://abc.com/abc".to_string()),
            serde_json::from_str(r#"{"TargetString":"https://abc.com/abc"}"#).unwrap()
        );
    }
    #[test]
    fn parse_api() {
        let api = Api {
            uri: Uri::TargetString("https://abc.com/abc".to_string()),
            method: HttpMethods(reqwest::Method::GET),
            headers: vec![
                ("a".to_string(), "b".to_string()),
                ("c".to_string(), "d".to_string()),
            ],
            data: None,
        };
        println!("{:?}", serde_json::to_string(&api));
        assert_eq!(
            api,
            serde_json::from_str(
                r#"{
                "uri" : {"TargetString":"https://abc.com/abc"},
                "method" : "GET",
                "headers": [
                    ["a", "b"],
                    ["c", "d"]
                ],
                "data" : null
            }"#
            )
            .unwrap()
        );
        let api_file = ApiFile{
            apis: HashMap::from([("ok".to_string(), api)])
        };
        println!("api_file: {:?}", serde_json::to_string(&api_file));
        assert_eq!(
            api_file,
            serde_json::from_str(
                r#"
                {
                    "apis": {
                        "ok": {
                            "uri" : {"TargetString":"https://abc.com/abc"},
                            "headers": [
                                ["a", "b"],
                                ["c", "d"]
                            ],
                            "data" : null 
                        }
                    }
                }
                "#
            )
            .unwrap()
        )
    }
}
