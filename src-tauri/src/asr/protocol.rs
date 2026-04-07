use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};

/// Build the 4-byte protocol header.
///
/// Byte 0: [protocol_version(4b) | header_size(4b)]
/// Byte 1: [message_type(4b) | flags(4b)]
/// Byte 2: [serialization(4b) | compression(4b)]
/// Byte 3: reserved
fn build_header(msg_type: u8, flags: u8, serialization: u8, compression: u8) -> [u8; 4] {
    [
        0x11, // version=1, header_size=1 (1*4=4 bytes)
        (msg_type << 4) | (flags & 0x0F),
        (serialization << 4) | (compression & 0x0F),
        0x00,
    ]
}

fn gzip_compress(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(data).map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}

fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = GzDecoder::new(data);
    let mut result = Vec::new();
    decoder
        .read_to_end(&mut result)
        .map_err(|e| e.to_string())?;
    Ok(result)
}

pub fn build_full_client_request(config_json: &str) -> Result<Vec<u8>, String> {
    let header = build_header(0x01, 0x00, 0x01, 0x01);
    let compressed = gzip_compress(config_json.as_bytes())?;
    let size = (compressed.len() as u32).to_be_bytes();

    let mut packet = Vec::with_capacity(4 + 4 + compressed.len());
    packet.extend_from_slice(&header);
    packet.extend_from_slice(&size);
    packet.extend_from_slice(&compressed);
    Ok(packet)
}

pub fn build_audio_request(audio_data: &[u8], is_last: bool) -> Result<Vec<u8>, String> {
    let flags = if is_last { 0x02 } else { 0x00 };
    let header = build_header(0x02, flags, 0x00, 0x01);
    let compressed = gzip_compress(audio_data)?;
    let size = (compressed.len() as u32).to_be_bytes();

    let mut packet = Vec::with_capacity(4 + 4 + compressed.len());
    packet.extend_from_slice(&header);
    packet.extend_from_slice(&size);
    packet.extend_from_slice(&compressed);
    Ok(packet)
}

#[derive(Debug)]
pub enum ServerMessage {
    Response {
        sequence: i32,
        payload: String,
        _is_last: bool,
    },
    Error {
        code: u32,
        message: String,
    },
}

/// Parse a server response following the binary protocol spec.
///
/// Based on the reference Python implementation:
/// - header_size (byte 0 low 4 bits) * 4 = actual header bytes
/// - After header, optional fields based on flags:
///   - bit 0 (0x01): sequence number present (4 bytes, signed i32 big-endian)
///   - bit 1 (0x02): is last package
///   - bit 2 (0x04): event field present (4 bytes)
/// - Then message-type-specific fields:
///   - SERVER_FULL_RESPONSE (0x09): payload_size(4B) + payload
///   - SERVER_ERROR (0x0F): error_code(4B) + error_size(4B) + error_msg
pub fn parse_server_response(data: &[u8]) -> Result<ServerMessage, String> {
    if data.len() < 4 {
        return Err("Response too short".to_string());
    }

    let header_size = (data[0] & 0x0F) as usize * 4;
    let msg_type = (data[1] >> 4) & 0x0F;
    let flags = data[1] & 0x0F;
    let _serialization = (data[2] >> 4) & 0x0F;
    let compression = data[2] & 0x0F;

    if data.len() < header_size {
        return Err("Response shorter than header".to_string());
    }

    // Start reading after the header
    let mut pos = header_size;

    // Parse optional fields based on flags
    let mut sequence: i32 = 0;
    let mut _is_last = false;

    // bit 0: sequence number present
    if flags & 0x01 != 0 {
        if data.len() < pos + 4 {
            return Err("Missing sequence field".to_string());
        }
        sequence = i32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        pos += 4;
    }

    // bit 1: is last package
    if flags & 0x02 != 0 {
        _is_last = true;
    }

    // bit 2: event field present
    if flags & 0x04 != 0 {
        if data.len() < pos + 4 {
            return Err("Missing event field".to_string());
        }
        pos += 4; // skip event
    }

    match msg_type {
        // SERVER_FULL_RESPONSE (0b1001 = 0x09)
        0x09 => {
            if data.len() < pos + 4 {
                // No payload — just an ack
                return Ok(ServerMessage::Response {
                    sequence,
                    payload: "{}".to_string(),
                    _is_last,
                });
            }

            let payload_size =
                u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                    as usize;
            pos += 4;

            if payload_size == 0 {
                return Ok(ServerMessage::Response {
                    sequence,
                    payload: "{}".to_string(),
                    _is_last,
                });
            }

            if data.len() < pos + payload_size {
                return Err(format!(
                    "Incomplete payload: expected {} bytes, got {}",
                    payload_size,
                    data.len() - pos
                ));
            }

            let payload_data = &data[pos..pos + payload_size];
            let json_bytes = if compression == 0x01 {
                gzip_decompress(payload_data)?
            } else {
                payload_data.to_vec()
            };

            let json_str = String::from_utf8(json_bytes).map_err(|e| e.to_string())?;
            Ok(ServerMessage::Response {
                sequence,
                payload: json_str,
                _is_last,
            })
        }

        // SERVER_ERROR_RESPONSE (0b1111 = 0x0F)
        0x0F => {
            if data.len() < pos + 8 {
                return Err("Error response too short".to_string());
            }

            let error_code =
                u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            pos += 4;

            let msg_size =
                u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                    as usize;
            pos += 4;

            let msg_data = if data.len() >= pos + msg_size {
                &data[pos..pos + msg_size]
            } else {
                &data[pos..]
            };

            // Decompress if gzipped
            let msg_bytes = if compression == 0x01 && !msg_data.is_empty() {
                gzip_decompress(msg_data).unwrap_or_else(|_| msg_data.to_vec())
            } else {
                msg_data.to_vec()
            };

            let error_msg = String::from_utf8_lossy(&msg_bytes).to_string();
            Ok(ServerMessage::Error {
                code: error_code,
                message: error_msg,
            })
        }

        _ => Err(format!("Unknown message type: 0x{:02X}", msg_type)),
    }
}
