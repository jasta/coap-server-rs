use crate::app::resource_builder::DiscoverableResource;
use coap_lite::link_format::{LinkAttributeWrite, LinkFormatWrite};
use coap_lite::ContentFormat;
use dyn_clone::DynClone;
use std::collections::HashMap;
use std::fmt::{Debug, Error, Write};

#[derive(Default, Debug)]
pub struct CoreLink {
    path: String,
    attributes: Vec<(&'static str, Box<dyn LinkAttributeValue<String>>)>,
}

impl CoreLink {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            attributes: Vec::new(),
        }
    }

    pub fn attr(&mut self, key: &'static str, value: impl LinkAttributeValue<String> + 'static) {
        self.attributes.push((key, Box::new(value)));
    }

    pub fn format_single_link(&self) -> String {
        let mut buf = String::new();
        let mut write = LinkFormatWrite::new(&mut buf);

        // Shouldn't be possible to yield Error because we're writing into a String
        write = self.write_link(write).unwrap();
        write.finish().unwrap();
        buf
    }

    fn write_link<'a>(
        &self,
        mut write: LinkFormatWrite<'a, String>,
    ) -> Result<LinkFormatWrite<'a, String>, Error> {
        let mut link = write.link(&self.path);
        for (key, value) in &self.attributes {
            link = value.write_to(link, key);
        }
        link.finish().map(|_| write)
    }
}

impl From<CoreLink> for DiscoverableResource {
    fn from(src: CoreLink) -> Self {
        let link_str = src.format_single_link();
        let attributes_as_string: HashMap<_, _> = src
            .attributes
            .into_iter()
            .map(|(k, v)| (k, v.format_for_comparison()))
            .collect();
        Self {
            link_str,
            attributes_as_string,
        }
    }
}

impl Clone for CoreLink {
    fn clone(&self) -> Self {
        let attributes: Vec<_> = self
            .attributes
            .iter()
            .map(|(key, value)| (*key, dyn_clone::clone_box(value.as_ref())))
            .collect();
        Self {
            path: self.path.clone(),
            attributes,
        }
    }
}

pub trait LinkAttributeValue<T>: DynClone + Debug {
    fn format_for_comparison(&self) -> String;
    fn write_to<'a, 'b>(
        &self,
        write: LinkAttributeWrite<'a, 'b, T>,
        key: &str,
    ) -> LinkAttributeWrite<'a, 'b, T>;
}

dyn_clone::clone_trait_object!(<T> LinkAttributeValue<T>);

impl<T: Write> LinkAttributeValue<T> for &str {
    fn format_for_comparison(&self) -> String {
        self.to_string()
    }

    fn write_to<'a, 'b>(
        &self,
        write: LinkAttributeWrite<'a, 'b, T>,
        key: &str,
    ) -> LinkAttributeWrite<'a, 'b, T> {
        write.attr_quoted(key, *self)
    }
}

impl<T: Write> LinkAttributeValue<T> for u32 {
    fn format_for_comparison(&self) -> String {
        self.to_string()
    }

    fn write_to<'a, 'b>(
        &self,
        write: LinkAttributeWrite<'a, 'b, T>,
        key: &str,
    ) -> LinkAttributeWrite<'a, 'b, T> {
        write.attr_u32(key, *self)
    }
}

impl<T: Write> LinkAttributeValue<T> for ContentFormat {
    fn format_for_comparison(&self) -> String {
        usize::from(*self).to_string()
    }

    fn write_to<'a, 'b>(
        &self,
        write: LinkAttributeWrite<'a, 'b, T>,
        key: &str,
    ) -> LinkAttributeWrite<'a, 'b, T> {
        let value = u32::try_from(usize::from(*self)).unwrap();
        write.attr_u32(key, value)
    }
}

impl<T: Write> LinkAttributeValue<T> for () {
    fn format_for_comparison(&self) -> String {
        "".to_string()
    }

    fn write_to<'a, 'b>(
        &self,
        write: LinkAttributeWrite<'a, 'b, T>,
        key: &str,
    ) -> LinkAttributeWrite<'a, 'b, T> {
        write.attr(key, "")
    }
}

#[cfg(test)]
mod tests {
    use crate::app::core_link::CoreLink;
    use coap_lite::link_format::{
        LinkFormatWrite, LINK_ATTR_CONTENT_FORMAT, LINK_ATTR_OBSERVABLE, LINK_ATTR_RESOURCE_TYPE,
    };
    use coap_lite::ContentFormat;

    #[test]
    fn test_multiple() {
        let mut a = CoreLink::new("/a");
        a.attr(LINK_ATTR_RESOURCE_TYPE, "a");
        a.attr(LINK_ATTR_CONTENT_FORMAT, ContentFormat::ApplicationJSON);

        let mut b = CoreLink::new("/b/whatever");
        b.attr(LINK_ATTR_OBSERVABLE, ());

        let out = format_links(vec![a, b]);

        assert_eq!(out, r#"</a>;rt="a";ct=50,</b/whatever>;obs="#);
    }

    #[test]
    fn test_clone() {
        let mut a = CoreLink::new("/a");
        a.attr(LINK_ATTR_RESOURCE_TYPE, "a");

        let b = a.clone();

        assert_eq!(format_links(vec![a]), format_links(vec![b]));
    }

    fn format_links(links: Vec<CoreLink>) -> String {
        let mut out = String::new();
        let mut write = LinkFormatWrite::new(&mut out);

        for link in links {
            write = link.write_link(write).unwrap();
        }

        out
    }
}
