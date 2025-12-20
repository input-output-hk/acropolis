use std::collections::{HashMap, HashSet};

use super::monitor::MonitorData;

/// Data flow graph showing producer/consumer relationships
#[derive(Debug, Clone)]
pub struct DataFlowGraph {
    /// topic -> list of modules that write to it
    pub producers: HashMap<String, Vec<String>>,
    /// topic -> list of modules that read from it
    pub consumers: HashMap<String, Vec<String>>,
    /// All unique topics
    pub topics: Vec<String>,
}

impl DataFlowGraph {
    /// Build a flow graph from monitor data
    pub fn from_monitor_data(data: &MonitorData) -> Self {
        let mut producers: HashMap<String, Vec<String>> = HashMap::new();
        let mut consumers: HashMap<String, Vec<String>> = HashMap::new();
        let mut topic_set: HashSet<String> = HashSet::new();

        for module in &data.modules {
            for write in &module.writes {
                topic_set.insert(write.topic.clone());
                producers.entry(write.topic.clone()).or_default().push(module.name.clone());
            }
            for read in &module.reads {
                topic_set.insert(read.topic.clone());
                consumers.entry(read.topic.clone()).or_default().push(module.name.clone());
            }
        }

        let mut topics: Vec<String> = topic_set.into_iter().collect();
        topics.sort();

        Self {
            producers,
            consumers,
            topics,
        }
    }

    /// Generate lines for ASCII display
    /// Format: producer(s) -> topic -> consumer(s)
    pub fn to_lines(&self) -> Vec<FlowLine> {
        self.topics
            .iter()
            .map(|topic| {
                let producers = self.producers.get(topic).cloned().unwrap_or_default();
                let consumers = self.consumers.get(topic).cloned().unwrap_or_default();

                FlowLine {
                    topic: topic.clone(),
                    producers,
                    consumers,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct FlowLine {
    pub topic: String,
    pub producers: Vec<String>,
    pub consumers: Vec<String>,
}
