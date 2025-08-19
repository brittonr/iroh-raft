//! Test runner for iroh-raft comprehensive test suite
//!
//! This module provides utilities for running the complete test suite including
//! unit tests, property tests, integration tests, and simulation tests.

#![cfg(test)]

use std::process::Command;
use std::time::{Duration, Instant};

/// Test suite configuration
#[derive(Debug, Clone)]
pub struct TestSuiteConfig {
    pub run_unit_tests: bool,
    pub run_property_tests: bool,
    pub run_integration_tests: bool,
    pub run_madsim_tests: bool,
    pub run_connection_pooling_tests: bool,
    pub run_zero_copy_tests: bool,
    pub run_graceful_shutdown_tests: bool,
    pub timeout: Duration,
}

impl Default for TestSuiteConfig {
    fn default() -> Self {
        Self {
            run_unit_tests: true,
            run_property_tests: true,
            run_integration_tests: true,
            run_madsim_tests: true,
            run_connection_pooling_tests: true,
            run_zero_copy_tests: true,
            run_graceful_shutdown_tests: true,
            timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Test result summary
#[derive(Debug, Clone)]
pub struct TestResults {
    pub total_tests: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration: Duration,
    pub test_categories: Vec<CategoryResult>,
}

/// Result for a specific test category
#[derive(Debug, Clone)]
pub struct CategoryResult {
    pub category: String,
    pub tests_run: usize,
    pub passed: usize,
    pub failed: usize,
    pub duration: Duration,
    pub details: Vec<String>,
}

impl TestResults {
    fn new() -> Self {
        Self {
            total_tests: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            duration: Duration::ZERO,
            test_categories: Vec::new(),
        }
    }
    
    fn add_category(&mut self, category: CategoryResult) {
        self.total_tests += category.tests_run;
        self.passed += category.passed;
        self.failed += category.failed;
        self.duration += category.duration;
        self.test_categories.push(category);
    }
    
    pub fn print_summary(&self) {
        println!("\n=== Test Suite Summary ===");
        println!("Total Tests: {}", self.total_tests);
        println!("Passed: {} ({:.1}%)", self.passed, 
                (self.passed as f64 / self.total_tests as f64) * 100.0);
        println!("Failed: {} ({:.1}%)", self.failed,
                (self.failed as f64 / self.total_tests as f64) * 100.0);
        println!("Skipped: {}", self.skipped);
        println!("Total Duration: {:?}", self.duration);
        
        println!("\n=== Category Breakdown ===");
        for category in &self.test_categories {
            println!("{}: {} tests, {} passed, {} failed, {:?}", 
                    category.category, category.tests_run, category.passed, 
                    category.failed, category.duration);
        }
        
        if self.failed > 0 {
            println!("\n=== Failed Test Details ===");
            for category in &self.test_categories {
                if category.failed > 0 {
                    println!("Category: {}", category.category);
                    for detail in &category.details {
                        if detail.contains("FAILED") {
                            println!("  {}", detail);
                        }
                    }
                }
            }
        }
    }
}

/// Test runner for iroh-raft comprehensive test suite
pub struct TestRunner {
    config: TestSuiteConfig,
}

impl TestRunner {
    pub fn new(config: TestSuiteConfig) -> Self {
        Self { config }
    }
    
    pub fn with_default_config() -> Self {
        Self::new(TestSuiteConfig::default())
    }
    
    /// Run the complete test suite
    pub async fn run_all_tests(&self) -> TestResults {
        let mut results = TestResults::new();
        let suite_start = Instant::now();
        
        println!("🚀 Starting iroh-raft comprehensive test suite...");
        
        // Run connection pooling tests
        if self.config.run_connection_pooling_tests {
            let category_result = self.run_connection_pooling_tests().await;
            results.add_category(category_result);
        }
        
        // Run zero-copy tests
        if self.config.run_zero_copy_tests {
            let category_result = self.run_zero_copy_tests().await;
            results.add_category(category_result);
        }
        
        // Run graceful shutdown tests
        if self.config.run_graceful_shutdown_tests {
            let category_result = self.run_graceful_shutdown_tests().await;
            results.add_category(category_result);
        }
        
        // Run integration tests
        if self.config.run_integration_tests {
            let category_result = self.run_integration_tests().await;
            results.add_category(category_result);
        }
        
        // Run property tests
        if self.config.run_property_tests {
            let category_result = self.run_property_tests().await;
            results.add_category(category_result);
        }
        
        // Run madsim tests
        if self.config.run_madsim_tests {
            let category_result = self.run_madsim_tests().await;
            results.add_category(category_result);
        }
        
        // Run unit tests
        if self.config.run_unit_tests {
            let category_result = self.run_unit_tests().await;
            results.add_category(category_result);
        }
        
        results.duration = suite_start.elapsed();
        results
    }
    
    async fn run_connection_pooling_tests(&self) -> CategoryResult {
        println!("🔗 Running connection pooling tests...");
        let start = Instant::now();
        
        let output = Command::new("cargo")
            .args(&["test", "--test", "connection_pooling_tests", "--", "--nocapture"])
            .output()
            .expect("Failed to run connection pooling tests");
        
        let duration = start.elapsed();
        self.parse_test_output("Connection Pooling", output, duration)
    }
    
    async fn run_zero_copy_tests(&self) -> CategoryResult {
        println!("⚡ Running zero-copy message tests...");
        let start = Instant::now();
        
        let output = Command::new("cargo")
            .args(&["test", "--test", "zero_copy_tests", "--", "--nocapture"])
            .output()
            .expect("Failed to run zero-copy tests");
        
        let duration = start.elapsed();
        self.parse_test_output("Zero-Copy Messages", output, duration)
    }
    
    async fn run_graceful_shutdown_tests(&self) -> CategoryResult {
        println!("🛑 Running graceful shutdown tests...");
        let start = Instant::now();
        
        let output = Command::new("cargo")
            .args(&["test", "--test", "graceful_shutdown_tests", "--", "--nocapture"])
            .output()
            .expect("Failed to run graceful shutdown tests");
        
        let duration = start.elapsed();
        self.parse_test_output("Graceful Shutdown", output, duration)
    }
    
    async fn run_integration_tests(&self) -> CategoryResult {
        println!("🔄 Running integration tests...");
        let start = Instant::now();
        
        let output = Command::new("cargo")
            .args(&["test", "--test", "integration_tests", "--", "--nocapture"])
            .output()
            .expect("Failed to run integration tests");
        
        let duration = start.elapsed();
        self.parse_test_output("Integration", output, duration)
    }
    
    async fn run_property_tests(&self) -> CategoryResult {
        println!("🎲 Running property-based tests...");
        let start = Instant::now();
        
        let output = Command::new("cargo")
            .args(&["test", "--test", "property_tests", "--features", "test-helpers", "--", "--nocapture"])
            .output()
            .expect("Failed to run property tests");
        
        let duration = start.elapsed();
        self.parse_test_output("Property-Based", output, duration)
    }
    
    async fn run_madsim_tests(&self) -> CategoryResult {
        println!("🌐 Running madsim distributed tests...");
        let start = Instant::now();
        
        let output = Command::new("cargo")
            .args(&["test", "--test", "madsim_distributed_tests", "--features", "madsim", "--", "--nocapture"])
            .output()
            .expect("Failed to run madsim tests");
        
        let duration = start.elapsed();
        self.parse_test_output("Madsim Distributed", output, duration)
    }
    
    async fn run_unit_tests(&self) -> CategoryResult {
        println!("🧪 Running unit tests...");
        let start = Instant::now();
        
        let output = Command::new("cargo")
            .args(&["test", "--lib", "--", "--nocapture"])
            .output()
            .expect("Failed to run unit tests");
        
        let duration = start.elapsed();
        self.parse_test_output("Unit Tests", output, duration)
    }
    
    fn parse_test_output(&self, category: &str, output: std::process::Output, duration: Duration) -> CategoryResult {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined_output = format!("{}\n{}", stdout, stderr);
        
        let mut tests_run = 0;
        let mut passed = 0;
        let mut failed = 0;
        let mut details = Vec::new();
        
        // Parse test output to extract statistics
        for line in combined_output.lines() {
            if line.contains("test result:") {
                // Example: "test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"
                if let Some(stats_part) = line.split("test result:").nth(1) {
                    if let Some(passed_str) = extract_number(stats_part, "passed") {
                        passed = passed_str;
                    }
                    if let Some(failed_str) = extract_number(stats_part, "failed") {
                        failed = failed_str;
                    }
                    tests_run = passed + failed;
                }
            } else if line.contains("FAILED") || line.contains("ERROR") {
                details.push(line.to_string());
            } else if line.contains("test ") && (line.contains("... ok") || line.contains("... FAILED")) {
                details.push(line.to_string());
            }
        }
        
        // If we couldn't parse the summary, count individual test lines
        if tests_run == 0 {
            let ok_count = combined_output.matches("... ok").count();
            let failed_count = combined_output.matches("... FAILED").count();
            passed = ok_count;
            failed = failed_count;
            tests_run = ok_count + failed_count;
        }
        
        println!("  ✅ {}: {} tests run, {} passed, {} failed", 
                category, tests_run, passed, failed);
        
        CategoryResult {
            category: category.to_string(),
            tests_run,
            passed,
            failed,
            duration,
            details,
        }
    }
}

fn extract_number(text: &str, keyword: &str) -> Option<usize> {
    if let Some(pos) = text.find(keyword) {
        let before_keyword = &text[..pos];
        if let Some(num_start) = before_keyword.rfind(|c: char| !c.is_ascii_digit()) {
            let num_str = &before_keyword[num_start + 1..];
            num_str.parse().ok()
        } else {
            before_keyword.parse().ok()
        }
    } else {
        None
    }
}

/// Convenience functions for running specific test categories
pub async fn run_connection_pooling_tests() -> CategoryResult {
    let runner = TestRunner::with_default_config();
    runner.run_connection_pooling_tests().await
}

pub async fn run_zero_copy_tests() -> CategoryResult {
    let runner = TestRunner::with_default_config();
    runner.run_zero_copy_tests().await
}

pub async fn run_graceful_shutdown_tests() -> CategoryResult {
    let runner = TestRunner::with_default_config();
    runner.run_graceful_shutdown_tests().await
}

pub async fn run_integration_tests() -> CategoryResult {
    let runner = TestRunner::with_default_config();
    runner.run_integration_tests().await
}

/// Main test runner that can be called from examples or scripts
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 iroh-raft Comprehensive Test Runner");
    println!("=====================================");
    
    let config = TestSuiteConfig::default();
    let runner = TestRunner::new(config);
    
    let results = runner.run_all_tests().await;
    results.print_summary();
    
    if results.failed > 0 {
        println!("\n❌ Some tests failed. Please review the failures above.");
        std::process::exit(1);
    } else {
        println!("\n✅ All tests passed successfully!");
    }
    
    Ok(())
}

#[cfg(test)]
mod test_runner_tests {
    use super::*;
    
    #[tokio::test]
    async fn test_runner_basic_functionality() {
        let config = TestSuiteConfig {
            run_unit_tests: false,
            run_property_tests: false,
            run_integration_tests: false,
            run_madsim_tests: false,
            run_connection_pooling_tests: true,
            run_zero_copy_tests: true,
            run_graceful_shutdown_tests: true,
            timeout: Duration::from_secs(60),
        };
        
        let runner = TestRunner::new(config);
        let results = runner.run_all_tests().await;
        
        // Should have run the three enabled test categories
        assert!(results.test_categories.len() >= 3);
        assert!(results.total_tests > 0);
    }
    
    #[test]
    fn test_extract_number() {
        assert_eq!(extract_number("15 passed; 0 failed", "passed"), Some(15));
        assert_eq!(extract_number("0 passed; 3 failed", "failed"), Some(3));
        assert_eq!(extract_number("no numbers here", "passed"), None);
    }
    
    #[test]
    fn test_test_results_summary() {
        let mut results = TestResults::new();
        
        let category1 = CategoryResult {
            category: "Test Category 1".to_string(),
            tests_run: 10,
            passed: 8,
            failed: 2,
            duration: Duration::from_secs(5),
            details: vec!["test1 ... ok".to_string(), "test2 ... FAILED".to_string()],
        };
        
        let category2 = CategoryResult {
            category: "Test Category 2".to_string(),
            tests_run: 5,
            passed: 5,
            failed: 0,
            duration: Duration::from_secs(3),
            details: vec!["test3 ... ok".to_string()],
        };
        
        results.add_category(category1);
        results.add_category(category2);
        
        assert_eq!(results.total_tests, 15);
        assert_eq!(results.passed, 13);
        assert_eq!(results.failed, 2);
        assert_eq!(results.duration, Duration::from_secs(8));
        assert_eq!(results.test_categories.len(), 2);
    }
}