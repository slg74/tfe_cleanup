use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde_json::Value;
use std::env;
use std::io::{self, Write};
use std::process::Command;
use chrono::{DateTime, Utc, Duration};
use csv::{Writer, Reader};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Obtain TFE token from environment
    let tfe_token = env::var("TFE_TOKEN").expect("TFE_TOKEN not set in environment");

    // Create HTTP client with authorization header
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", tfe_token))?);

    // Login to Terraform Enterprise (assuming login is just setting up the client)
    // If there's a specific login endpoint, you'd make a request here

    // Get list of TFE accounts
    let accounts_response = client.get("https://app.terraform.io/api/v2/organizations")
        .headers(headers.clone())
        .send()
        .await?
        .json::<Value>()
        .await?;

    let mut old_inactive_accounts = Vec::new();
    let ninety_days_ago = Utc::now() - Duration::days(90);

    if let Some(accounts) = accounts_response["data"].as_array() {
        for account in accounts {
            let last_activity = account["attributes"]["last-activity-at"].as_str().unwrap_or("");
            if let Ok(last_activity_date) = DateTime::parse_from_rfc3339(last_activity) {
                if last_activity_date < ninety_days_ago {
                    old_inactive_accounts.push(account);
                }
            }
        }
    }

    // Print to stdout
    println!("Accounts older than 90 days with no activity:");
    for account in &old_inactive_accounts {
        println!("{}", account["attributes"]["name"]);
    }

    // Write to CSV
    let mut wtr = Writer::from_path("old_inactive_accounts.csv")?;
    wtr.write_record(&["Name", "Last Activity"])?;

    for account in old_inactive_accounts {
        wtr.write_record(&[
            account["attributes"]["name"].as_str().unwrap_or(""),
            account["attributes"]["last-activity-at"].as_str().unwrap_or(""),
        ])?;
    }

    wtr.flush()?;

    println!("CSV file 'old_inactive_accounts.csv' has been created.");

    print!("Do you want to perform Terraform cleanup? (y/n): ");
    io::stdout().flush()?;

    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input)?;

    if user_input.trim().to_lowercase() == "y" {
        println!("Proceeding with Terraform Enterprise Account cleanup..."); 
        perform_terraform_cleanup()?;
    } else {
        println!("Cleanup skipped.");
    }

    Ok(())
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
