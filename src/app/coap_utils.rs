use std::collections::HashMap;

use coap_lite::{CoapOption, CoapRequest};
use coap_lite::error::IncompatibleOptionValueFormat;
use coap_lite::option_value::OptionValueType;

use crate::app::CoapError;

pub fn request_get_queries<Endpoint>(request: &CoapRequest<Endpoint>) -> HashMap<String, String> {
  request.message.get_options_as::<OptionValueQuery>(CoapOption::UriQuery)
      .map_or_else(
        || { HashMap::new() },
        |options| {
          options.into_iter()
              .filter_map(Result::ok)
              .map(|query| {
                (query.key, query.value)
              })
              .collect::<HashMap<_, _>>()
        })
}

pub struct OptionValueQuery {
  key: String,
  value: String,
}

impl From<OptionValueQuery> for Vec<u8> {
  fn from(option_value: OptionValueQuery) -> Self {
    format!("{}={}", option_value.key, option_value.value).into_bytes()
  }
}

impl TryFrom<Vec<u8>> for OptionValueQuery {
  type Error = IncompatibleOptionValueFormat;

  fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
    Self::do_try_from(value)
        .map_err(|e| IncompatibleOptionValueFormat { message: e.to_string() })
  }
}

impl OptionValueType for OptionValueQuery {
}

impl OptionValueQuery {
  fn do_try_from(value: Vec<u8>) -> Result<Self, CoapError> {
    let mut key_value_iter = value.split(|&c| c == b'=');
    let key_vec = key_value_iter.next().unwrap().to_vec();
    let value_vec = key_value_iter.next()
        .ok_or_else(|| CoapError::bad_request("Missing '=', not a valid query string"))?
        .to_vec();
    let key = String::from_utf8(key_vec)
        .map_err(|e| CoapError::bad_request(format!("Key is not UTF-8: {e}")))?;
    let value = String::from_utf8(value_vec)
        .map_err(|e| CoapError::bad_request(format!("Value is not UTF-8: {e}")))?;

    Ok(OptionValueQuery { key, value })
  }
}