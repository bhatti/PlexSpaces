// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Performance Metrics for V8 Isolates Comparison
//
// Tracks comprehensive performance metrics including:
// - Throughput (events/second)
// - Latency (p50, p95, p99)
// - Memory footprint
// - CPU utilization
// - Coordination vs computation time
// - Scalability metrics

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Instant;

/// Comprehensive performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Test name/identifier
    pub test_name: String,
    
    /// Total events processed
    pub total_events: u64,
    
    /// Total duration
    pub total_duration_ms: u64,
    
    /// Throughput (events/second)
    pub throughput_events_per_sec: f64,
    
    /// Latency metrics (microseconds)
    pub latency_p50_us: u64,
    pub latency_p95_us: u64,
    pub latency_p99_us: u64,
    pub latency_avg_us: u64,
    pub latency_max_us: u64,
    
    /// Coordination time (message passing, waiting)
    pub coordination_time_ms: u64,
    
    /// Computation time (actual processing)
    pub computation_time_ms: u64,
    
    /// Granularity ratio (computation / coordination)
    pub granularity_ratio: f64,
    
    /// Efficiency (computation / total)
    pub efficiency: f64,
    
    /// Memory metrics (bytes)
    pub memory_peak_bytes: u64,
    pub memory_avg_bytes: u64,
    
    /// Message counts
    pub messages_sent: u64,
    pub messages_received: u64,
    
    /// Actor counts
    pub actors_created: u64,
    
    /// Error counts
    pub errors: u64,
    
    /// I/O metrics (for I/O benchmarks)
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub read_throughput_mb_per_sec: f64,
    pub write_throughput_mb_per_sec: f64,
    pub log_entries_processed: u64,
    
    /// Timestamp when metrics were collected
    pub timestamp: u64,
}

impl PerformanceMetrics {
    pub fn new(test_name: String) -> Self {
        Self {
            test_name,
            total_events: 0,
            total_duration_ms: 0,
            throughput_events_per_sec: 0.0,
            latency_p50_us: 0,
            latency_p95_us: 0,
            latency_p99_us: 0,
            latency_avg_us: 0,
            latency_max_us: 0,
            coordination_time_ms: 0,
            computation_time_ms: 0,
            granularity_ratio: 0.0,
            efficiency: 0.0,
            memory_peak_bytes: 0,
            memory_avg_bytes: 0,
            messages_sent: 0,
            messages_received: 0,
            actors_created: 0,
            errors: 0,
            bytes_read: 0,
            bytes_written: 0,
            read_throughput_mb_per_sec: 0.0,
            write_throughput_mb_per_sec: 0.0,
            log_entries_processed: 0,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
    
    pub fn calculate_derived(&mut self) {
        // Calculate throughput
        if self.total_duration_ms > 0 {
            self.throughput_events_per_sec = 
                (self.total_events as f64 * 1000.0) / self.total_duration_ms as f64;
        }
        
        // Calculate granularity ratio
        if self.coordination_time_ms > 0 {
            self.granularity_ratio = 
                self.computation_time_ms as f64 / self.coordination_time_ms as f64;
        } else {
            self.granularity_ratio = f64::INFINITY;
        }
        
        // Calculate efficiency
        let total_time = self.coordination_time_ms + self.computation_time_ms;
        if total_time > 0 {
            self.efficiency = self.computation_time_ms as f64 / total_time as f64;
        }
        
        // Calculate I/O throughput
        if self.total_duration_ms > 0 {
            let duration_sec = self.total_duration_ms as f64 / 1000.0;
            if duration_sec > 0.0 {
                self.read_throughput_mb_per_sec = (self.bytes_read as f64 / 1_048_576.0) / duration_sec;
                self.write_throughput_mb_per_sec = (self.bytes_written as f64 / 1_048_576.0) / duration_sec;
            }
        }
    }
}

/// Metrics collector for tracking performance during benchmarks
pub struct MetricsCollector {
    test_name: String,
    start_time: Instant,
    latencies: VecDeque<u64>, // microseconds
    coordination_times: Vec<u64>, // milliseconds
    computation_times: Vec<u64>, // milliseconds
    total_events: u64,
    messages_sent: u64,
    messages_received: u64,
    actors_created: u64,
    errors: u64,
    memory_samples: Vec<u64>, // bytes
    bytes_read: u64,
    bytes_written: u64,
    log_entries_processed: u64,
}

impl MetricsCollector {
    pub fn new(test_name: String) -> Self {
        Self {
            test_name,
            start_time: Instant::now(),
            latencies: VecDeque::new(),
            coordination_times: Vec::new(),
            computation_times: Vec::new(),
            total_events: 0,
            messages_sent: 0,
            messages_received: 0,
            actors_created: 0,
            errors: 0,
            memory_samples: Vec::new(),
            bytes_read: 0,
            bytes_written: 0,
            log_entries_processed: 0,
        }
    }
    
    pub fn record_event(&mut self, latency_us: u64) {
        self.total_events += 1;
        self.latencies.push_back(latency_us);
        
        // Keep only last 10000 samples to avoid memory issues
        if self.latencies.len() > 10000 {
            self.latencies.pop_front();
        }
    }
    
    pub fn record_coordination(&mut self, time_ms: u64) {
        self.coordination_times.push(time_ms);
    }
    
    pub fn record_computation(&mut self, time_ms: u64) {
        self.computation_times.push(time_ms);
    }
    
    pub fn record_message_sent(&mut self) {
        self.messages_sent += 1;
    }
    
    pub fn record_message_received(&mut self) {
        self.messages_received += 1;
    }
    
    pub fn record_actor_created(&mut self) {
        self.actors_created += 1;
    }
    
    pub fn record_error(&mut self) {
        self.errors += 1;
    }
    
    pub fn record_bytes_read(&mut self, bytes: u64) {
        self.bytes_read += bytes;
    }
    
    pub fn record_bytes_written(&mut self, bytes: u64) {
        self.bytes_written += bytes;
    }
    
    pub fn record_log_entry(&mut self) {
        self.log_entries_processed += 1;
    }
    
    pub fn sample_memory(&mut self) {
        // Get current memory usage (RSS)
        let memory = get_memory_usage();
        self.memory_samples.push(memory);
    }
    
    pub fn finalize(&mut self) -> PerformanceMetrics {
        let total_duration = self.start_time.elapsed();
        let total_duration_ms = total_duration.as_millis() as u64;
        
        // Calculate latency percentiles
        let mut sorted_latencies: Vec<u64> = self.latencies.iter().copied().collect();
        sorted_latencies.sort();
        
        let latency_p50 = percentile(&sorted_latencies, 50);
        let latency_p95 = percentile(&sorted_latencies, 95);
        let latency_p99 = percentile(&sorted_latencies, 99);
        let latency_avg = if !sorted_latencies.is_empty() {
            sorted_latencies.iter().sum::<u64>() / sorted_latencies.len() as u64
        } else {
            0
        };
        let latency_max = sorted_latencies.last().copied().unwrap_or(0);
        
        // Calculate coordination and computation times
        let coordination_time_ms = self.coordination_times.iter().sum::<u64>();
        let computation_time_ms = self.computation_times.iter().sum::<u64>();
        
        // Calculate memory metrics
        let memory_peak = self.memory_samples.iter().max().copied().unwrap_or(0);
        let memory_avg = if !self.memory_samples.is_empty() {
            self.memory_samples.iter().sum::<u64>() / self.memory_samples.len() as u64
        } else {
            0
        };
        
        let mut metrics = PerformanceMetrics {
            test_name: self.test_name.clone(),
            total_events: self.total_events,
            total_duration_ms,
            throughput_events_per_sec: 0.0, // Will be calculated
            latency_p50_us: latency_p50,
            latency_p95_us: latency_p95,
            latency_p99_us: latency_p99,
            latency_avg_us: latency_avg,
            latency_max_us: latency_max,
            coordination_time_ms,
            computation_time_ms,
            granularity_ratio: 0.0, // Will be calculated
            efficiency: 0.0, // Will be calculated
            memory_peak_bytes: memory_peak,
            memory_avg_bytes: memory_avg,
            messages_sent: self.messages_sent,
            messages_received: self.messages_received,
            actors_created: self.actors_created,
            errors: self.errors,
            bytes_read: self.bytes_read,
            bytes_written: self.bytes_written,
            read_throughput_mb_per_sec: 0.0, // Will be calculated
            write_throughput_mb_per_sec: 0.0, // Will be calculated
            log_entries_processed: self.log_entries_processed,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        
        metrics.calculate_derived();
        metrics
    }
}

/// Calculate percentile from sorted vector
/// Uses the "nearest rank" method: index = (p/100) * (n-1)
/// This matches the test expectations where percentile values align with array values
fn percentile(sorted: &[u64], p: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    // Use nearest rank method: index = (p/100) * (n-1)
    // This gives us the element at the p-th percentile position
    let n = sorted.len();
    let index = (p * (n - 1)) / 100;
    sorted[index.min(n - 1)]
}

/// Get current memory usage (RSS) in bytes
fn get_memory_usage() -> u64 {
    // Try to read from /proc/self/status on Linux
    #[cfg(target_os = "linux")]
    {
        if let Ok(contents) = std::fs::read_to_string("/proc/self/status") {
            for line in contents.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(value) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = value.parse::<u64>() {
                            return kb * 1024; // Convert KB to bytes
                        }
                    }
                }
            }
        }
    }
    
    // Fallback: use a simple approximation
    // In production, use proper memory tracking libraries
    0
}

impl PerformanceMetrics {
    /// Print comprehensive performance report
    pub fn print_report(&self) {
        println!("\n╔════════════════════════════════════════════════════════════════╗");
        println!("║  Performance Metrics: {}", format!("{:.<40}", self.test_name));
        println!("╚════════════════════════════════════════════════════════════════╝");
        println!();
        
        // Throughput
        println!("📊 Throughput:");
        println!("  Events Processed:     {:>12}", self.total_events);
        println!("  Duration:            {:>12} ms", self.total_duration_ms);
        println!("  Throughput:          {:>12.2} events/sec", self.throughput_events_per_sec);
        println!("  Throughput:          {:>12.2} events/min", self.throughput_events_per_sec * 60.0);
        println!();
        
        // Latency
        println!("⏱️  Latency (microseconds):");
        println!("  Average (p50):       {:>12} μs", self.latency_avg_us);
        println!("  Median (p50):        {:>12} μs", self.latency_p50_us);
        println!("  p95:                 {:>12} μs", self.latency_p95_us);
        println!("  p99:                 {:>12} μs", self.latency_p99_us);
        println!("  Max:                 {:>12} μs", self.latency_max_us);
        println!();
        
        // Timing breakdown
        println!("⏳ Timing Breakdown:");
        println!("  Computation Time:    {:>12} ms", self.computation_time_ms);
        println!("  Coordination Time:   {:>12} ms", self.coordination_time_ms);
        println!("  Total Time:          {:>12} ms", 
            self.computation_time_ms + self.coordination_time_ms);
        println!();
        
        // Performance ratios
        println!("📈 Performance Ratios:");
        println!("  Granularity Ratio:   {:>12.2}× (compute/coordinate)", self.granularity_ratio);
        
        if self.granularity_ratio < 10.0 {
            println!("    ⚠️  WARNING: Ratio < 10×! Coordination overhead too high!");
        } else if self.granularity_ratio < 100.0 {
            println!("    ✓  Acceptable (>10×), but could be optimized");
        } else {
            println!("    ✅ Excellent (>100×)! Optimal granularity");
        }
        
        println!("  Efficiency:          {:>12.1}% (compute/total)", self.efficiency * 100.0);
        
        if self.efficiency < 0.8 {
            println!("    ⚠️  Efficiency < 80%! Too much coordination overhead");
        } else {
            println!("    ✅ Good efficiency (>80%)");
        }
        println!();
        
        // Memory
        println!("💾 Memory:");
        println!("  Peak Memory:         {:>12} bytes ({:.2} MB)", 
            self.memory_peak_bytes, self.memory_peak_bytes as f64 / 1_048_576.0);
        println!("  Average Memory:      {:>12} bytes ({:.2} MB)", 
            self.memory_avg_bytes, self.memory_avg_bytes as f64 / 1_048_576.0);
        println!();
        
        // I/O metrics (if available)
        if self.bytes_read > 0 || self.bytes_written > 0 {
            println!("📁 I/O Performance:");
            println!("  Bytes Read:          {:>12} bytes ({:.2} MB)", 
                self.bytes_read, self.bytes_read as f64 / 1_048_576.0);
            println!("  Bytes Written:       {:>12} bytes ({:.2} MB)", 
                self.bytes_written, self.bytes_written as f64 / 1_048_576.0);
            println!("  Read Throughput:     {:>12.2} MB/sec", self.read_throughput_mb_per_sec);
            println!("  Write Throughput:   {:>12.2} MB/sec", self.write_throughput_mb_per_sec);
            println!("  Log Entries:         {:>12}", self.log_entries_processed);
            println!();
        }
        
        // Communication
        println!("📨 Communication:");
        println!("  Messages Sent:       {:>12}", self.messages_sent);
        println!("  Messages Received:   {:>12}", self.messages_received);
        println!("  Actors Created:      {:>12}", self.actors_created);
        println!("  Errors:              {:>12}", self.errors);
        println!();
        
        // Production readiness assessment
        println!("🏭 Production Readiness:");
        let mut score = 0;
        let mut max_score = 0;
        
        // Throughput check
        max_score += 1;
        if self.throughput_events_per_sec >= 10_000.0 {
            println!("  ✅ Throughput: {} events/sec (excellent)", self.throughput_events_per_sec as u64);
            score += 1;
        } else if self.throughput_events_per_sec >= 1_000.0 {
            println!("  ✓  Throughput: {} events/sec (good)", self.throughput_events_per_sec as u64);
            score += 1;
        } else {
            println!("  ⚠️  Throughput: {} events/sec (needs optimization)", 
                self.throughput_events_per_sec as u64);
        }
        
        // Latency check
        max_score += 1;
        if self.latency_p95_us < 10_000 {
            println!("  ✅ Latency p95: {} μs (excellent)", self.latency_p95_us);
            score += 1;
        } else if self.latency_p95_us < 100_000 {
            println!("  ✓  Latency p95: {} μs (good)", self.latency_p95_us);
            score += 1;
        } else {
            println!("  ⚠️  Latency p95: {} μs (needs optimization)", self.latency_p95_us);
        }
        
        // Efficiency check
        max_score += 1;
        if self.efficiency >= 0.9 {
            println!("  ✅ Efficiency: {:.1}% (excellent)", self.efficiency * 100.0);
            score += 1;
        } else if self.efficiency >= 0.8 {
            println!("  ✓  Efficiency: {:.1}% (good)", self.efficiency * 100.0);
            score += 1;
        } else {
            println!("  ⚠️  Efficiency: {:.1}% (needs optimization)", self.efficiency * 100.0);
        }
        
        // Memory check
        max_score += 1;
        let memory_mb = self.memory_peak_bytes as f64 / 1_048_576.0;
        if memory_mb < 100.0 {
            println!("  ✅ Memory: {:.2} MB (excellent)", memory_mb);
            score += 1;
        } else if memory_mb < 500.0 {
            println!("  ✓  Memory: {:.2} MB (good)", memory_mb);
            score += 1;
        } else {
            println!("  ⚠️  Memory: {:.2} MB (consider optimization)", memory_mb);
        }
        
        println!();
        println!("  Overall Score: {}/{}", score, max_score);
        if score == max_score {
            println!("  ✅ Production Ready!");
        } else if score >= max_score / 2 {
            println!("  ✓  Production Ready with optimizations");
        } else {
            println!("  ⚠️  Needs optimization before production");
        }
        
        println!();
        println!("╔════════════════════════════════════════════════════════════════╗");
        println!("║  Metrics Collection Complete                                  ║");
        println!("╚════════════════════════════════════════════════════════════════╝");
    }
    
    /// Export metrics as JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_metrics_collector() {
        let mut collector = MetricsCollector::new("test".to_string());
        collector.record_event(100);
        collector.record_event(200);
        collector.record_coordination(10);
        collector.record_computation(100);
        
        let metrics = collector.finalize();
        assert_eq!(metrics.total_events, 2);
        assert_eq!(metrics.coordination_time_ms, 10);
        assert_eq!(metrics.computation_time_ms, 100);
    }
    
    #[test]
    fn test_percentile() {
        let sorted = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        // For 10 elements (indices 0-9):
        // 50th percentile: index = (50 * 9) / 100 = 4, value = 50
        assert_eq!(percentile(&sorted, 50), 50);
        // 95th percentile: index = (95 * 9) / 100 = 8, value = 90
        assert_eq!(percentile(&sorted, 95), 90);
        // 99th percentile: index = (99 * 9) / 100 = 8, value = 90
        assert_eq!(percentile(&sorted, 99), 90);
    }
}

