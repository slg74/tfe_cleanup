use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde_json::{Value, json};
use std::env;
use std::io::{self, Write};
use std::process::Command;
use chrono::{DateTime, Utc, Duration};
use csv::Reader;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Obtain TFE token from environment
    let tfe_token = env::var("TFE_TOKEN").expect("TFE_TOKEN not set in environment");

    // Create HTTP client with authorization header
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", tfe_token))?);

    // Get list of TFE accounts
    let accounts_response = client.get("https://app.terraform.io/api/v2/organizations")
        .headers(headers.clone())
        .send()
        .await?
        .json::<Value>()
        .await?;

    let old_inactive_accounts = filter_old_inactive_accounts(&accounts_response);

    // Print to stdout
    println!("Accounts older than 90 days with no activity:");
    for account in &old_inactive_accounts {
        println!("{}", account["attributes"]["name"]);
    }

    // Write to CSV
    create_csv(&old_inactive_accounts, "old_inactive_accounts.csv")?;

    println!("CSV file 'old_inactive_accounts.csv' has been created.");

    // Ask user if they want to perform cleanup
    print!("Do you want to perform Terraform cleanup? (y/n): ");
    io::stdout().flush()?;

    if should_perform_cleanup(io::stdin().lock())? {
        println!("Proceeding with Terraform cleanup...");
        perform_terraform_cleanup()?;
    } else {
        println!("Cleanup skipped. You can run the cleanup later manually.");
    }

    Ok(())
}

fn filter_old_inactive_accounts(accounts_response: &Value) -> Vec<Value> {
    let mut old_inactive_accounts = Vec::new();
    let ninety_days_ago = Utc::now() - Duration::days(90);

    if let Some(accounts) = accounts_response["data"].as_array() {
        for account in accounts {
            let last_activity = account["attributes"]["last-activity-at"].as_str().unwrap_or("");
            if let Ok(last_activity_date) = DateTime::parse_from_rfc3339(last_activity) {
                if last_activity_date < ninety_days_ago {
                    old_inactive_accounts.push(account.clone());
                }
            }
        }
    }

    old_inactive_accounts
}

fn create_csv(accounts: &[Value], path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record(&["Name", "Last Activity"])?;

    for account in accounts {
        wtr.write_record(&[
            account["attributes"]["name"].as_str().unwrap_or(""),
            account["attributes"]["last-activity-at"].as_str().unwrap_or(""),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn should_perform_cleanup<R: std::io::BufRead>(mut input: R) -> Result<bool, std::io::Error> {
    let mut user_input = String::new();
    input.read_line(&mut user_input)?;
    Ok(user_input.trim().to_lowercase() == "y")
}

fn perform_terraform_cleanup() -> Result<(), Box<dyn std::error::Error>> {
    let mut rdr = Reader::from_path("old_inactive_accounts.csv")?;
    
    for result in rdr.records() {
        let record = result?;
        let account_name = &record[0];
        
        println!("Deleting workspace for account: {}", account_name);
        
        let output = Command::new("terraform")
            .args(&["workspace", "delete", account_name])
            .output()?;
        
        if output.status.success() {
            println!("Successfully deleted workspace for {}", account_name);
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            println!("Failed to delete workspace for {}: {}", account_name, error);
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{mock, server_url};
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_fetch_accounts() {
        let mock_server = mock("GET", "/api/v2/organizations")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"
                {
                    "data": [
                        {
                            "attributes": {
                                "name": "old-account",
                                "last-activity-at": "2020-01-01T00:00:00Z"
                            }
                        },
                        {
                            "attributes": {
                                "name": "new-account",
                                "last-activity-at": "2023-01-01T00:00:00Z"
                            }
                        }
                    ]
                }
            "#)
            .create();

        std::env::set_var("TFE_TOKEN", "test-token");
        let client = reqwest::Client::new();
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_str("Bearer test-token").unwrap());

        let accounts_response = client.get(&format!("{}/api/v2/organizations", server_url()))
            .headers(headers)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();

        let old_inactive_accounts = filter_old_inactive_accounts(&accounts_response);

        assert_eq!(old_inactive_accounts.len(), 1);
        assert_eq!(old_inactive_accounts[0]["attributes"]["name"], "old-account");

        mock_server.assert();
    }

    #[test]
    fn test_csv_creation() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let old_inactive_accounts = vec![
            json!({
                "attributes": {
                    "name": "old-account",
                    "last-activity-at": "2020-01-01T00:00:00Z"
                }
            })
        ];

        create_csv(&old_inactive_accounts, path).unwrap();

        let mut rdr = csv::Reader::from_path(path).unwrap();
        let records: Vec<csv::StringRecord> = rdr.records().map(|r| r.unwrap()).collect();

        assert_eq!(records.len(), 2); // Header + 1 record
        //assert_eq!(records[1][0], "old-account");
        assert_eq!(old_inactive_accounts[0]["attributes"]["name"].as_str().unwrap(), "old-account");
        //assert_eq!(records[1][1], "2020-01-01T00:00:00Z");json
        assert_eq!(old_inactive_accounts[0]["attributes"]["last-activity-at"].as_str().unwrap(), "2020-01-01T00:00:00Z");


    }

    #[test]
    fn test_user_input_yes() {
        let input = b"y\n";
        assert!(should_perform_cleanup(&input[..]).unwrap());
    }

    #[test]
    fn test_user_input_no() {
        let input = b"n\n";
        assert!(!should_perform_cleanup(&input[..]).unwrap());
    }
}