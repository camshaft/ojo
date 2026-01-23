use ojo_builder::{Builder, EventInfo, ValueType};

fn main() {
    Builder::new()
        .add_event(EventInfo {
            name: "OFFSET_SENT".to_string(),
            category: "Packet".to_string(),
            description: "Packet was sent to the network".to_string(),
            value_type: ValueType::RangeBytes,
        })
        .add_event(EventInfo {
            name: "OFFSET_RETRANSMITTED".to_string(),
            category: "Packet".to_string(),
            description: "Packet was retransmitted".to_string(),
            value_type: ValueType::RangeBytes,
        })
        .add_event(EventInfo {
            name: "OFFSET_ACKED".to_string(),
            category: "Packet".to_string(),
            description: "Packet was acknowledged by peer".to_string(),
            value_type: ValueType::RangeBytes,
        })
        .add_event(EventInfo {
            name: "STREAM_OPENED".to_string(),
            category: "Stream".to_string(),
            description: "New stream created".to_string(),
            value_type: ValueType::Identifier,
        })
        .add_event(EventInfo {
            name: "CWND_UPDATED".to_string(),
            category: "Congestion".to_string(),
            description: "Congestion window updated".to_string(),
            value_type: ValueType::Bytes,
        })
        .add_event(EventInfo {
            name: "PACKET_LOST_TIMEOUT".to_string(),
            category: "Packet".to_string(),
            description: "Packet lost due to timeout".to_string(),
            value_type: ValueType::Identifier,
        })
        .generate("events");
}
