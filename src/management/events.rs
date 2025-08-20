//! Real-time event streaming for iroh-raft clusters
//!
//! This module provides P2P event streaming capabilities for monitoring
//! cluster state changes, leadership events, and other important notifications.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use futures_util::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::error::Result;
use super::{MemberRole, HealthStatus, RaftState};

/// Event types for real-time monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClusterEvent {
    /// Leadership change events
    LeaderElected { 
        new_leader: u64, 
        term: u64,
        previous_leader: Option<u64>,
        timestamp: SystemTime,
    },
    LeadershipTransferred {
        from_leader: u64,
        to_leader: u64,
        term: u64,
        duration: Duration,
        timestamp: SystemTime,
    },
    LeaderSteppedDown {
        leader: u64,
        term: u64,
        reason: String,
        timestamp: SystemTime,
    },

    /// Member management events
    MemberAdded { 
        node_id: u64, 
        role: MemberRole,
        address: String,
        timestamp: SystemTime,
    },
    MemberRemoved { 
        node_id: u64,
        reason: String,
        timestamp: SystemTime,
    },
    MemberRoleChanged {
        node_id: u64,
        old_role: MemberRole,
        new_role: MemberRole,
        timestamp: SystemTime,
    },
    MemberHealthChanged { 
        node_id: u64, 
        old_status: HealthStatus,
        new_status: HealthStatus,
        timestamp: SystemTime,
    },

    /// Configuration and operational events
    ConfigurationChanged { 
        version: u64,
        changes: Vec<ConfigChange>,
        timestamp: SystemTime,
    },
    SnapshotCompleted { 
        index: u64, 
        term: u64,
        size_bytes: u64,
        duration: Duration,
        timestamp: SystemTime,
    },
    SnapshotFailed {
        index: u64,
        term: u64,
        error: String,
        timestamp: SystemTime,
    },

    /// Custom application events
    Custom {
        event_type: String,
        data: serde_json::Value,
        timestamp: SystemTime,
    },
}

/// Configuration change details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChange {
    pub field: String,
    pub old_value: serde_json::Value,
    pub new_value: serde_json::Value,
}

/// Event subscription filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
    /// Event types to include (if empty, include all)
    pub event_types: Vec<String>,
    /// Filter by specific node IDs
    pub node_ids: Vec<u64>,
    /// Time range filter
    pub since: Option<SystemTime>,
    pub until: Option<SystemTime>,
}

impl Default for EventFilter {
    fn default() -> Self {
        Self {
            event_types: Vec::new(),
            node_ids: Vec::new(),
            since: None,
            until: None,
        }
    }
}

impl EventFilter {
    /// Check if an event matches this filter
    pub fn matches(&self, event: &ClusterEvent) -> bool {
        // Check event type filter
        if !self.event_types.is_empty() {
            let event_type = event.event_type();
            if !self.event_types.contains(&event_type) {
                return false;
            }
        }

        // Check node ID filter
        if !self.node_ids.is_empty() {
            if let Some(node_id) = event.node_id() {
                if !self.node_ids.contains(&node_id) {
                    return false;
                }
            }
        }

        // Check time range filter
        let event_time = event.timestamp();
        if let Some(since) = self.since {
            if event_time < since {
                return false;
            }
        }
        if let Some(until) = self.until {
            if event_time > until {
                return false;
            }
        }

        true
    }
}

impl ClusterEvent {
    /// Get the event type as a string
    pub fn event_type(&self) -> String {
        match self {
            ClusterEvent::LeaderElected { .. } => "leader_elected".to_string(),
            ClusterEvent::LeadershipTransferred { .. } => "leadership_transferred".to_string(),
            ClusterEvent::LeaderSteppedDown { .. } => "leader_stepped_down".to_string(),
            ClusterEvent::MemberAdded { .. } => "member_added".to_string(),
            ClusterEvent::MemberRemoved { .. } => "member_removed".to_string(),
            ClusterEvent::MemberRoleChanged { .. } => "member_role_changed".to_string(),
            ClusterEvent::MemberHealthChanged { .. } => "member_health_changed".to_string(),
            ClusterEvent::ConfigurationChanged { .. } => "configuration_changed".to_string(),
            ClusterEvent::SnapshotCompleted { .. } => "snapshot_completed".to_string(),
            ClusterEvent::SnapshotFailed { .. } => "snapshot_failed".to_string(),
            ClusterEvent::Custom { event_type, .. } => event_type.clone(),
        }
    }

    /// Get the timestamp of this event
    pub fn timestamp(&self) -> SystemTime {
        match self {
            ClusterEvent::LeaderElected { timestamp, .. } => *timestamp,
            ClusterEvent::LeadershipTransferred { timestamp, .. } => *timestamp,
            ClusterEvent::LeaderSteppedDown { timestamp, .. } => *timestamp,
            ClusterEvent::MemberAdded { timestamp, .. } => *timestamp,
            ClusterEvent::MemberRemoved { timestamp, .. } => *timestamp,
            ClusterEvent::MemberRoleChanged { timestamp, .. } => *timestamp,
            ClusterEvent::MemberHealthChanged { timestamp, .. } => *timestamp,
            ClusterEvent::ConfigurationChanged { timestamp, .. } => *timestamp,
            ClusterEvent::SnapshotCompleted { timestamp, .. } => *timestamp,
            ClusterEvent::SnapshotFailed { timestamp, .. } => *timestamp,
            ClusterEvent::Custom { timestamp, .. } => *timestamp,
        }
    }

    /// Get the node ID associated with this event (if any)
    pub fn node_id(&self) -> Option<u64> {
        match self {
            ClusterEvent::LeaderElected { new_leader, .. } => Some(*new_leader),
            ClusterEvent::LeadershipTransferred { to_leader, .. } => Some(*to_leader),
            ClusterEvent::LeaderSteppedDown { leader, .. } => Some(*leader),
            ClusterEvent::MemberAdded { node_id, .. } => Some(*node_id),
            ClusterEvent::MemberRemoved { node_id, .. } => Some(*node_id),
            ClusterEvent::MemberRoleChanged { node_id, .. } => Some(*node_id),
            ClusterEvent::MemberHealthChanged { node_id, .. } => Some(*node_id),
            _ => None,
        }
    }

    /// Create a leader elected event
    pub fn leader_elected(new_leader: u64, term: u64, previous_leader: Option<u64>) -> Self {
        ClusterEvent::LeaderElected {
            new_leader,
            term,
            previous_leader,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a member added event
    pub fn member_added(node_id: u64, role: MemberRole, address: String) -> Self {
        ClusterEvent::MemberAdded {
            node_id,
            role,
            address,
            timestamp: SystemTime::now(),
        }
    }

    /// Create a health changed event
    pub fn health_changed(node_id: u64, old_status: HealthStatus, new_status: HealthStatus) -> Self {
        ClusterEvent::MemberHealthChanged {
            node_id,
            old_status,
            new_status,
            timestamp: SystemTime::now(),
        }
    }
}

/// Stream of cluster events with filtering capabilities
pub struct EventStream {
    receiver: mpsc::UnboundedReceiver<ClusterEvent>,
    filter: EventFilter,
    subscription_id: [u8; 16],
}

impl EventStream {
    /// Create a new event stream
    pub fn new(receiver: mpsc::UnboundedReceiver<ClusterEvent>, subscription_id: [u8; 16]) -> Self {
        Self {
            receiver,
            filter: EventFilter::default(),
            subscription_id,
        }
    }

    /// Create an event stream with a filter
    pub fn with_filter(
        receiver: mpsc::UnboundedReceiver<ClusterEvent>,
        subscription_id: [u8; 16],
        filter: EventFilter,
    ) -> Self {
        Self {
            receiver,
            filter,
            subscription_id,
        }
    }

    /// Get the subscription ID
    pub fn subscription_id(&self) -> [u8; 16] {
        self.subscription_id
    }

    /// Update the event filter
    pub fn set_filter(&mut self, filter: EventFilter) {
        self.filter = filter;
    }

    /// Try to receive the next event without blocking
    pub fn try_next(&mut self) -> Option<ClusterEvent> {
        while let Ok(event) = self.receiver.try_recv() {
            if self.filter.matches(&event) {
                return Some(event);
            }
            // Skip events that don't match the filter
        }
        None
    }

    /// Close the event stream
    pub fn close(&mut self) {
        self.receiver.close();
    }
}

impl Stream for EventStream {
    type Item = ClusterEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.receiver.poll_recv(cx) {
                Poll::Ready(Some(event)) => {
                    if self.filter.matches(&event) {
                        return Poll::Ready(Some(event));
                    }
                    // Continue polling for events that match the filter
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Event broadcaster for distributing events to multiple subscribers
#[derive(Debug)]
pub struct EventBroadcaster {
    subscribers: HashMap<[u8; 16], mpsc::UnboundedSender<ClusterEvent>>,
}

impl EventBroadcaster {
    /// Create a new event broadcaster
    pub fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
        }
    }

    /// Add a new subscriber
    pub fn subscribe(&mut self, subscription_id: [u8; 16]) -> mpsc::UnboundedReceiver<ClusterEvent> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.subscribers.insert(subscription_id, sender);
        receiver
    }

    /// Remove a subscriber
    pub fn unsubscribe(&mut self, subscription_id: &[u8; 16]) {
        self.subscribers.remove(subscription_id);
    }

    /// Broadcast an event to all subscribers
    pub fn broadcast(&mut self, event: ClusterEvent) {
        let mut closed_subscribers = Vec::new();

        for (subscription_id, sender) in &self.subscribers {
            if let Err(_) = sender.send(event.clone()) {
                // Subscriber channel is closed
                closed_subscribers.push(*subscription_id);
            }
        }

        // Clean up closed subscribers
        for subscription_id in closed_subscribers {
            self.subscribers.remove(&subscription_id);
        }
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }
}

impl Default for EventBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

/// Event history store for querying past events
#[derive(Debug)]
pub struct EventHistory {
    events: Vec<ClusterEvent>,
    max_events: usize,
}

impl EventHistory {
    /// Create a new event history store
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Vec::new(),
            max_events,
        }
    }

    /// Add an event to the history
    pub fn add_event(&mut self, event: ClusterEvent) {
        self.events.push(event);
        
        // Keep only the most recent events
        if self.events.len() > self.max_events {
            self.events.drain(0..self.events.len() - self.max_events);
        }
    }

    /// Query events with a filter
    pub fn query(&self, filter: &EventFilter) -> Vec<&ClusterEvent> {
        self.events.iter()
            .filter(|event| filter.matches(event))
            .collect()
    }

    /// Get the most recent events
    pub fn recent_events(&self, count: usize) -> Vec<&ClusterEvent> {
        let start = if self.events.len() > count {
            self.events.len() - count
        } else {
            0
        };
        self.events[start..].iter().collect()
    }

    /// Clear all events
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Get total event count
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}