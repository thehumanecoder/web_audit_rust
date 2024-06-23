extern crate reqwest;
extern crate select;
extern crate regex;
extern crate chrono;
extern crate robotparser;

use reqwest::blocking::Client;
use select::document::Document;
use select::predicate::{Attr, Name, Predicate};
use regex::Regex;
use std::collections::HashMap;
use std::error::Error;
use chrono::Utc;
use std::env;

fn fetch_url(client: &Client, url: &str) -> Result<String, Box<dyn Error>> {
    let res = client.get(url).send()?.text()?;
    Ok(res)
}

fn get_response_time(client: &Client, url: &str) -> Result<u128, Box<dyn Error>> {
    let start = Utc::now();
    client.get(url).send()?;
    let duration = Utc::now().signed_duration_since(start).num_milliseconds() as u128;
    Ok(duration)
}

fn has_schema_markup(html: &str) -> bool {
    // Attempt to parse the document, return false if parsing fails
    let document = match Document::from_read(html.as_bytes()) {
        Ok(doc) => doc,
        Err(_) => return false,
    };

    // Check for JSON-LD schema
    let json_ld = document.find(Name("script").and(Attr("type", "application/ld+json"))).any(|script| {
        script.text().trim().starts_with('{')
    });

    // Check for Microdata schema
    let microdata = document.find(Attr("itemscope", ())).next().is_some();

    // Check for RDFa schema
    let rdfa = document.find(Attr("typeof", ())).next().is_some();

    // Return true if any of the schema markups are present
    json_ld || microdata || rdfa
}

fn get_robots_txt(client: &Client, url: &str) -> Option<String> {
    fetch_url(client, &format!("{}/robots.txt", url)).ok()
}

fn is_valid_robots_txt(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    let mut valid = true;
    for line in lines {
        if !(line.starts_with("User-agent:") || line.starts_with("Disallow:") || line.starts_with("Allow:") || line.trim().is_empty() || line.starts_with("#")) {
            valid = false;
            break;
        }
    }
    valid
}

fn has_sitemap_xml(client: &Client, url: &str) -> bool {
    fetch_url(client, &format!("{}/sitemap.xml", url)).is_ok()
}

fn get_canonical(html: &str) -> Option<String> {
    let document = Document::from(html);
    document.find(Name("link").and(Attr("rel", "canonical")))
        .next()
        .and_then(|n| n.attr("href").map(|href| href.to_string()))
}

fn get_broken_links(client: &Client, html: &str, base_url: &str) -> (Vec<String>, Vec<String>) {
    let document = Document::from(html);
    let mut broken_links = Vec::new();
    let mut broken_link_pages = Vec::new();
    for link in document.find(Name("a").and(Attr("href", ()))) {
        if let Some(href) = link.attr("href") {
            let url = if href.starts_with('/') {
                format!("{}{}", base_url, href)
            } else {
                href.to_string()
            };
            if let Err(_) = client.head(&url).send() {
                broken_links.push(url.clone());
                broken_link_pages.push(href.to_string());
            }
        }
    }
    (broken_links, broken_link_pages)
}

fn has_amp(html: &str) -> bool {
    // Attempt to parse the document, return false if parsing fails
    let document = match Document::from_read(html.as_bytes()) {
        Ok(doc) => doc,
        Err(_) => return false,
    };

    // Check for <link rel="amphtml" href="...">, <html amp>, <html ⚡>, and AMP script in a single traversal
    let mut amphtml_link = false;
    let mut amp_html_tag = false;
    let mut amp_script = false;

    for node in document.find(Name("link").or(Name("html")).or(Name("script"))) {
        if node.is(Name("link")) && node.attr("rel") == Some("amphtml") {
            amphtml_link = true;
        }
        if node.is(Name("html")) && (node.attr("amp").is_some() || node.attr("⚡").is_some()) {
            amp_html_tag = true;
        }
        if node.is(Name("script")) && node.attr("src") == Some("https://cdn.ampproject.org/v0.js") {
            amp_script = true;
        }

        // If any condition is met, no need to continue searching
        if amphtml_link || amp_html_tag || amp_script {
            return true;
        }
    }

    // Return true if any of the conditions are met
    amphtml_link || amp_html_tag || amp_script
}

fn is_responsive(html: &str) -> bool {
    let document = Document::from(html);
    document.find(Name("meta").and(Attr("name", "viewport"))).next().is_some()
}

fn has_google_analytics(html: &str) -> bool {
    let re = Regex::new(r"UA-\d+-\d+").unwrap();
    re.is_match(html)
}

fn is_indexed(html: &str) -> bool {
    let document = Document::from(html);
    if let Some(meta) = document.find(Name("meta").and(Attr("name", "robots"))).next() {
        return !meta.attr("content").unwrap_or("").to_lowercase().contains("noindex");
    }
    true
}

fn has_search_console(html: &str) -> bool {
    let document = Document::from(html);
    document.find(Name("meta").and(Attr("name", "google-site-verification"))).next().is_some()
}

fn get_website_details(url: &str) -> HashMap<String, Vec<String>> {
    let client = Client::new();
    let mut details = HashMap::new();

    match fetch_url(&client, url) {
        Ok(html) => {
            details.insert("Schema Markup".to_string(), vec![if has_schema_markup(&html) { "Found".to_string() } else { "Not Found".to_string() }]);
            
            if let Some(robots_txt) = get_robots_txt(&client, url) {
                details.insert("Robots.txt".to_string(), vec![robots_txt.clone()]);
                details.insert("Robots.txt Status".to_string(), vec![if is_valid_robots_txt(&robots_txt) { "Valid".to_string() } else { "Invalid".to_string() }]);
            } else {
                details.insert("Robots.txt".to_string(), vec!["Not Found".to_string()]);
                details.insert("Robots.txt Status".to_string(), vec!["Not Found".to_string()]);
            }
            
            details.insert("Sitemap.xml".to_string(), vec![if has_sitemap_xml(&client, url) { "Found".to_string() } else { "Not Found".to_string() }]);
            details.insert("Canonical Tags".to_string(), vec![get_canonical(&html).unwrap_or_default()]);
            details.insert("AMP".to_string(), vec![has_amp(&html).to_string()]);
            details.insert("Responsive".to_string(), vec![is_responsive(&html).to_string()]);
            details.insert("Google Analytics".to_string(), vec![has_google_analytics(&html).to_string()]);
            details.insert("Search Console".to_string(), vec![has_search_console(&html).to_string()]);
            details.insert("Search Console Status".to_string(), vec![if details.get("Search Console").unwrap().contains(&"true".to_string()) { "Present".to_string() } else { "Absent".to_string() }]);
            
            let (broken_links, broken_link_pages) = get_broken_links(&client, &html, url);
            details.insert("Broken Links".to_string(), broken_links);
            details.insert("Broken Link Pages".to_string(), broken_link_pages);
            details.insert("Index Pages".to_string(), vec![if is_indexed(&html) { url.to_string() } else { String::new() }]);
            details.insert("Non Index Pages".to_string(), vec![if !is_indexed(&html) { url.to_string() } else { String::new() }]);

            match get_response_time(&client, url) {
                Ok(time) => {
                    details.insert("Desktop Load Time".to_string(), vec![time.to_string()]);
                    details.insert("Mobile Load Time".to_string(), vec![time.to_string()]);
                    details.insert("Tablet Load Time".to_string(), vec![time.to_string()]);
                    details.insert("Desktop Load Time Result".to_string(), vec![if time < 2000 { "Good".to_string() } else if time < 4000 { "Moderate".to_string() } else { "Poor".to_string() }]);
                    details.insert("Mobile Load Time Result".to_string(), vec![if time < 2000 { "Good".to_string() } else if time < 4000 { "Moderate".to_string() } else { "Poor".to_string() }]);
                    details.insert("Tablet Load Time Result".to_string(), vec![if time < 2000 { "Good".to_string() } else if time < 4000 { "Moderate".to_string() } else { "Poor".to_string() }]);
                    details.insert("Load Time Grade".to_string(), vec![if time < 2000 { "A".to_string() } else if time < 4000 { "B".to_string() } else { "C".to_string() }]);
                },
                Err(_) => {
                    details.insert("Desktop Load Time".to_string(), vec!["N/A".to_string()]);
                    details.insert("Mobile Load Time".to_string(), vec!["N/A".to_string()]);
                    details.insert("Tablet Load Time".to_string(), vec!["N/A".to_string()]);
                    details.insert("Desktop Load Time Result".to_string(), vec!["N/A".to_string()]);
                    details.insert("Mobile Load Time Result".to_string(), vec!["N/A".to_string()]);
                    details.insert("Tablet Load Time Result".to_string(), vec!["N/A".to_string()]);
                    details.insert("Load Time Grade".to_string(), vec!["N/A".to_string()]);
                }
            }
        }
        Err(_) => {
            details.insert("error".to_string(), vec!["Failed to retrieve website content".to_string()]);
        }
    }

    details
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <url>", args[0]);
        return;
    }
    let url = &args[1];
    let website_details = get_website_details(url);
    for (key, value) in website_details.iter() {
        println!("{}: {:?}", key, value);
    }
}
