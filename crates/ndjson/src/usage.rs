use serde::{Deserialize, Serialize};

use crate::types::FinalResult;

/// Accumulated token and cost data across multiple turns.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UsageStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    pub turns: u32,
}

impl UsageStats {
    /// Ingest a `FinalResult` event, updating all counters.
    pub fn ingest(&mut self, result: &FinalResult) {
        if let Some(cost) = result.total_cost_usd {
            self.total_cost_usd += cost;
        }
        if let Some(turns) = result.num_turns {
            self.turns = self.turns.saturating_add(turns as u32);
        }
        if let Some(ref usage) = result.usage {
            if let Some(n) = usage.input_tokens {
                self.input_tokens = self.input_tokens.saturating_add(n);
            }
            if let Some(n) = usage.output_tokens {
                self.output_tokens = self.output_tokens.saturating_add(n);
            }
            if let Some(n) = usage.cache_read_input_tokens {
                self.cache_read_tokens = self.cache_read_tokens.saturating_add(n);
            }
            if let Some(n) = usage.cache_creation_input_tokens {
                self.cache_creation_tokens = self.cache_creation_tokens.saturating_add(n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FinalResult;

    #[test]
    fn ingest_accumulates_cost() {
        let mut stats = UsageStats::default();

        let r1 = FinalResult {
            total_cost_usd: Some(0.001),
            num_turns: Some(1),
            usage: Some(crate::types::UsageSummary {
                input_tokens: Some(100),
                output_tokens: Some(50),
                ..Default::default()
            }),
            ..Default::default()
        };

        let r2 = FinalResult {
            total_cost_usd: Some(0.002),
            num_turns: Some(2),
            usage: Some(crate::types::UsageSummary {
                input_tokens: Some(200),
                output_tokens: Some(80),
                ..Default::default()
            }),
            ..Default::default()
        };

        stats.ingest(&r1);
        stats.ingest(&r2);

        assert!((stats.total_cost_usd - 0.003).abs() < 1e-9);
        assert_eq!(stats.input_tokens, 300);
        assert_eq!(stats.output_tokens, 130);
        assert_eq!(stats.turns, 3);
    }
}
