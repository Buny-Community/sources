// Wuxiaworld's API is gRPC-web (protobuf over HTTP POST) at api2.wuxiaworld.com.
// The message layouts come from the protobuf-ts descriptors in the site's own
// bundles (`wuxiaworld.api.v2.*`). Only the fields the source reads are decoded;
// everything else is skipped by wire type, so new fields don't break parsing.

use buny::{
	Result,
	alloc::{String, Vec, string::ToString},
	imports::net::Request,
	prelude::*,
};

const API_URL: &str = "https://api2.wuxiaworld.com/wuxiaworld.api.v2.";

// Protobuf wire types.
const VARINT: u8 = 0;
const FIXED64: u8 = 1;
const LEN: u8 = 2;
const FIXED32: u8 = 5;

/// A protobuf message being encoded.
#[derive(Default)]
pub struct Writer(Vec<u8>);

impl Writer {
	#[cfg(test)]
	pub fn as_bytes(&self) -> &[u8] {
		&self.0
	}

	fn varint(&mut self, mut value: u64) {
		loop {
			let byte = (value & 0x7f) as u8;
			value >>= 7;
			if value == 0 {
				self.0.push(byte);
				return;
			}
			self.0.push(byte | 0x80);
		}
	}

	fn key(&mut self, field: u32, wire: u8) {
		self.varint(((field as u64) << 3) | wire as u64);
	}

	/// An int32 or enum field. Negative values are sign-extended to 64 bits,
	/// as protobuf requires (NovelItem.Status.All is -1).
	pub fn int32(&mut self, field: u32, value: i32) -> &mut Self {
		self.key(field, VARINT);
		self.varint(value as i64 as u64);
		self
	}

	pub fn bytes(&mut self, field: u32, value: &[u8]) -> &mut Self {
		self.key(field, LEN);
		self.varint(value.len() as u64);
		self.0.extend_from_slice(value);
		self
	}

	pub fn string(&mut self, field: u32, value: &str) -> &mut Self {
		self.bytes(field, value.as_bytes())
	}

	pub fn message(&mut self, field: u32, value: &Writer) -> &mut Self {
		self.bytes(field, &value.0)
	}

	/// A `google.protobuf.StringValue` wrapper.
	pub fn string_value(&mut self, field: u32, value: &str) -> &mut Self {
		let mut inner = Writer::default();
		inner.string(1, value);
		self.message(field, &inner)
	}

	/// A `google.protobuf.Int32Value` wrapper.
	pub fn int32_value(&mut self, field: u32, value: i32) -> &mut Self {
		let mut inner = Writer::default();
		inner.int32(1, value);
		self.message(field, &inner)
	}
}

/// One decoded field value.
#[derive(Clone, Copy)]
pub enum Value<'a> {
	Varint(u64),
	Bytes(&'a [u8]),
	// fixed64 / double: nothing reads one, it is only skipped.
	Fixed64,
	// fixed32 / sfixed32 / float
	Fixed32(u32),
}

/// A view over an encoded protobuf message.
#[derive(Clone, Copy, Default)]
pub struct Message<'a>(&'a [u8]);

impl<'a> Message<'a> {
	pub fn new(data: &'a [u8]) -> Self {
		Self(data)
	}

	/// Every field in wire order. Stops at the first malformed field.
	pub fn fields(self) -> Fields<'a> {
		Fields { data: self.0 }
	}

	fn values(self, field: u32) -> impl Iterator<Item = Value<'a>> {
		self.fields()
			.filter(move |(no, _)| *no == field)
			.map(|(_, v)| v)
	}

	// Scalars keep the last occurrence, as protobuf parsers do.
	fn last(&self, field: u32) -> Option<Value<'a>> {
		self.values(field).last()
	}

	pub fn varint(&self, field: u32) -> Option<u64> {
		match self.last(field)? {
			Value::Varint(v) => Some(v),
			_ => None,
		}
	}

	pub fn int32(&self, field: u32) -> Option<i32> {
		self.varint(field).map(|v| v as i32)
	}

	pub fn int64(&self, field: u32) -> Option<i64> {
		self.varint(field).map(|v| v as i64)
	}

	pub fn bool(&self, field: u32) -> bool {
		self.varint(field).is_some_and(|v| v != 0)
	}

	pub fn bytes(&self, field: u32) -> Option<&'a [u8]> {
		match self.last(field)? {
			Value::Bytes(b) => Some(b),
			_ => None,
		}
	}

	pub fn str(&self, field: u32) -> Option<&'a str> {
		self.bytes(field).and_then(|b| core::str::from_utf8(b).ok())
	}

	pub fn message(&self, field: u32) -> Option<Message<'a>> {
		self.bytes(field).map(Message)
	}

	pub fn messages(self, field: u32) -> impl Iterator<Item = Message<'a>> {
		self.values(field).filter_map(|v| match v {
			Value::Bytes(b) => Some(Message(b)),
			_ => None,
		})
	}

	pub fn strs(self, field: u32) -> impl Iterator<Item = &'a str> {
		self.values(field).filter_map(|v| match v {
			Value::Bytes(b) => core::str::from_utf8(b).ok(),
			_ => None,
		})
	}

	/// The `value` of a `google.protobuf.StringValue` wrapper. An empty wrapper
	/// is how the API sends an empty string, so it reads as None too.
	pub fn string_value(&self, field: u32) -> Option<&'a str> {
		self.message(field)?.str(1).filter(|s| !s.is_empty())
	}

	/// The `value` of a `google.protobuf.Int32Value` wrapper (0 when the wrapper
	/// is present but empty, which is how protobuf encodes 0).
	pub fn int32_value(&self, field: u32) -> Option<i32> {
		self.message(field).map(|m| m.int32(1).unwrap_or(0))
	}

	/// A `wuxiaworld.api.v2.DecimalValue`: whole `units` plus `nanos` (10^-9).
	pub fn decimal(&self, field: u32) -> Option<f64> {
		let m = self.message(field)?;
		let units = m.int64(1).unwrap_or(0);
		let nanos = match m.last(2) {
			Some(Value::Fixed32(n)) => n as i32,
			_ => 0,
		};
		Some(units as f64 + nanos as f64 / 1e9)
	}

	/// The `seconds` of a `google.protobuf.Timestamp`.
	pub fn timestamp(&self, field: u32) -> Option<i64> {
		self.message(field)?.int64(1)
	}
}

pub struct Fields<'a> {
	data: &'a [u8],
}

fn read_varint(data: &[u8]) -> Option<(u64, usize)> {
	let mut value = 0u64;
	for (i, byte) in data.iter().enumerate().take(10) {
		value |= ((byte & 0x7f) as u64) << (7 * i);
		if byte & 0x80 == 0 {
			return Some((value, i + 1));
		}
	}
	None
}

impl<'a> Iterator for Fields<'a> {
	type Item = (u32, Value<'a>);

	fn next(&mut self) -> Option<Self::Item> {
		if self.data.is_empty() {
			return None;
		}
		let parsed = (|| {
			let (key, n) = read_varint(self.data)?;
			let rest = &self.data[n..];
			let field = (key >> 3) as u32;
			let (value, used) = match (key & 7) as u8 {
				VARINT => {
					let (v, n) = read_varint(rest)?;
					(Value::Varint(v), n)
				}
				FIXED64 => {
					rest.get(..8)?;
					(Value::Fixed64, 8)
				}
				LEN => {
					let (len, n) = read_varint(rest)?;
					let end = n.checked_add(usize::try_from(len).ok()?)?;
					(Value::Bytes(rest.get(n..end)?), end)
				}
				FIXED32 => {
					let b: [u8; 4] = rest.get(..4)?.try_into().ok()?;
					(Value::Fixed32(u32::from_le_bytes(b)), 4)
				}
				_ => return None,
			};
			Some((field, value, n + used))
		})();
		match parsed {
			Some((field, value, used)) => {
				self.data = &self.data[used..];
				Some((field, value))
			}
			None => {
				self.data = &[];
				None
			}
		}
	}
}

// grpc-status codes worth their own message.
fn status_message(code: i32, message: Option<&str>) -> String {
	let reason = match code {
		5 => "not found",
		7 | 16 => "access denied",
		8 => "rate limited",
		12 => "method not available",
		14 => "service unavailable",
		_ => "request failed",
	};
	match message.map(str::trim).filter(|m| !m.is_empty()) {
		Some(m) => format!("Wuxiaworld API {reason} (grpc-status {code}): {m}"),
		None => format!("Wuxiaworld API {reason} (grpc-status {code})"),
	}
}

// Percent-decodes a grpc-message value (the spec percent-encodes it).
fn decode_message(raw: &str) -> String {
	let bytes = raw.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'%'
			&& let Some(hex) = raw.get(i + 1..i + 3)
			&& let Ok(b) = u8::from_str_radix(hex, 16)
		{
			out.push(b);
			i += 3;
			continue;
		}
		out.push(bytes[i]);
		i += 1;
	}
	String::from_utf8_lossy(&out).into_owned()
}

/// Splits a gRPC-web body into its message and trailer frames. Each frame is a
/// flag byte (bit 7 set for trailers), a big-endian u32 length, then the bytes.
fn frames(body: &[u8]) -> (Vec<&[u8]>, Vec<&[u8]>) {
	let (mut messages, mut trailers) = (Vec::new(), Vec::new());
	let mut rest = body;
	while rest.len() >= 5 {
		let flag = rest[0];
		let len = u32::from_be_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize;
		let Some(frame) = rest.get(5..5 + len) else {
			break;
		};
		if flag & 0x80 != 0 {
			trailers.push(frame);
		} else {
			messages.push(frame);
		}
		rest = &rest[5 + len..];
	}
	(messages, trailers)
}

// Reads `grpc-status` / `grpc-message` out of a trailer frame, which is
// formatted like HTTP/1 headers ("grpc-status: 0\r\n").
fn trailer_status(trailers: &[&[u8]]) -> (Option<i32>, Option<String>) {
	let (mut status, mut message) = (None, None);
	for frame in trailers {
		let text = String::from_utf8_lossy(frame);
		for line in text.split("\r\n") {
			let Some((name, value)) = line.split_once(':') else {
				continue;
			};
			match name.trim().to_ascii_lowercase().as_str() {
				"grpc-status" => status = value.trim().parse().ok(),
				"grpc-message" => message = Some(decode_message(value.trim())),
				_ => {}
			}
		}
	}
	(status, message)
}

/// Calls a unary gRPC-web method (`Service/Method`) and returns the raw reply
/// message. A reply for something that doesn't exist (an unknown slug) is an
/// empty message with status 0, so callers check for the fields they need.
pub fn call(method: &str, request: &Writer) -> Result<Vec<u8>> {
	let mut body = Vec::with_capacity(request.0.len() + 5);
	body.push(0);
	body.extend_from_slice(&(request.0.len() as u32).to_be_bytes());
	body.extend_from_slice(&request.0);

	let response = Request::post(format!("{API_URL}{method}"))?
		.header("Content-Type", "application/grpc-web+proto")
		.header("Accept", "application/grpc-web+proto")
		.header("X-Grpc-Web", "1")
		.header("Origin", "https://www.wuxiaworld.com")
		.header("Referer", "https://www.wuxiaworld.com/")
		.body(body)
		.send()?;

	// An error is sent "trailers-only": the status is an HTTP header and the
	// body is empty.
	if let Some(code) = response
		.get_header("grpc-status")
		.and_then(|s| s.trim().parse::<i32>().ok())
		&& code != 0
	{
		let message = response
			.get_header("grpc-message")
			.map(|m| decode_message(&m));
		bail!("{}", status_message(code, message.as_deref()));
	}
	let status_code = response.status_code();
	if !(200..300).contains(&status_code) {
		bail!("Wuxiaworld API returned HTTP {status_code}");
	}

	let data = response.get_data()?;
	let (messages, trailers) = frames(&data);
	let (status, message) = trailer_status(&trailers);
	if let Some(code) = status
		&& code != 0
	{
		bail!("{}", status_message(code, message.as_deref()));
	}
	match messages.first() {
		Some(frame) => Ok(frame.to_vec()),
		None if status == Some(0) => Ok(Vec::new()),
		None => bail!("Invalid response from the Wuxiaworld API"),
	}
}

/// Owned copy of a string field, trimmed.
pub fn owned(s: Option<&str>) -> Option<String> {
	s.map(str::trim)
		.filter(|s| !s.is_empty())
		.map(ToString::to_string)
}
