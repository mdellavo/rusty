extern crate reqwest;

pub fn get_reqw_client() -> reqwest::blocking::Client {
    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    return client;
}
