use reqwest::{StatusCode,Client};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, ACCEPT, CONTENT_TYPE};
use std::error::Error;
use serde_json::json;
use serde::Deserialize;

use std::env;

#[derive(Deserialize)]
struct User {
    id: i64,
    username: String,
}

#[derive(Deserialize)]
struct LoginResponse {
    access_token: String,
    refresh_token: String,
    user: User,
}

#[derive(Deserialize)]
struct Card {
    name: String,
}

#[derive(Deserialize)]
struct SearchResult {
    card: Card,
    quantity: i64,
}

#[derive(Deserialize)]
struct SearchResponse {
    results: Vec<SearchResult>
}

pub async fn search(_name: String) -> Result<String, Box<dyn Error>> {
    let client = Client::new();

    println!("Logging in to archidekt");
    let resp = client
        .post("https://archidekt.com/api/rest-auth/login/")
        .header(CONTENT_TYPE, "application/json")
        .json(
            &json!({
                "email": env::var("ARCHIDEKT_TUCKER_EMAIL").expect("Expected a token in the environment"),
                "password": env::var("ARCHIDEKT_TUCKER_PASSWORD").expect("Expected a token in the environment")
            })
        )
        .send()
        .await?;

    let login_response = match resp.status() {
            StatusCode::OK => {
                let login_response: LoginResponse = resp.json::<LoginResponse>().await?;
                Ok::<LoginResponse, Box<dyn Error>>(login_response)
            }
            status => Err(format!("Archidekt login failed with status code {}",status).into()),
        }?;

    println!("Searching library of user id {} for term {}",login_response.user.id,_name);
    let resp = client
        .get(format!("https://www.archidekt.com/api/collection/?cardName={}", _name))
        .header(AUTHORIZATION, format!("JWT {}",login_response.access_token))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .send()
        .await?;

    let search_response = match resp.status() {
            StatusCode::OK => {
                let search_response: SearchResponse = resp.json::<SearchResponse>().await?;
                Ok::<SearchResponse, Box<dyn Error>>(search_response)
            }
            status => Err(format!("Archidekt collection search failed with status code {}",status).into()),
        }?;

    let mut response_text = String::from(format!("Tucker's collection has `{}` matches for the search term `{}`:\n", search_response.results.len(),_name));


    for result in search_response.results {
        response_text.push_str(format!("{}\t{}\n",result.quantity,result.card.name).as_str());
    }

    Ok(response_text)
}