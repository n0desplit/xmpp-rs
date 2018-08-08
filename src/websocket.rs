// Copyright (c) 2018 Emmanuel Gil Peyrot <linkmauve@linkmauve.fr>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use jid::Jid;

generate_element!(
    /// The stream opening for WebSocket.
    Open, "open", WEBSOCKET,
    attributes: [
        /// The JID of the entity opening this stream.
        from: Option<Jid> = "from" => optional,

        /// The JID of the entity receiving this stream opening.
        to: Option<Jid> = "to" => optional,

        /// The id of the stream, used for authentication challenges.
        id: Option<String> = "id" => optional,

        /// The XMPP version used during this stream.
        version: Option<String> = "version" => optional,

        /// The default human language for all subsequent stanzas, which will
        /// be transmitted to other entities for better localisation.
        xml_lang: Option<String> = "xml:lang" => optional,
    ]
);

impl Open {
    /// Creates a simple client→server `<open/>` element.
    pub fn new(to: Jid) -> Open {
        Open {
            from: None,
            to: Some(to),
            id: None,
            version: Some(String::from("1.0")),
            xml_lang: None,
        }
    }

    /// Sets the [@from](#structfield.from) attribute on this `<open/>`
    /// element.
    pub fn with_from(mut self, from: Jid) -> Open {
        self.from = Some(from);
        self
    }

    /// Sets the [@id](#structfield.id) attribute on this `<open/>` element.
    pub fn with_id(mut self, id: String) -> Open {
        self.id = Some(id);
        self
    }

    /// Sets the [@xml:lang](#structfield.xml_lang) attribute on this `<open/>`
    /// element.
    pub fn with_lang(mut self, xml_lang: String) -> Open {
        self.xml_lang = Some(xml_lang);
        self
    }

    /// Checks whether the version matches the expected one.
    pub fn is_version(&self, version: &str) -> bool {
        match self.version {
            None => false,
            Some(ref self_version) => self_version == &String::from(version),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use try_from::TryFrom;
    use minidom::Element;

    #[test]
    fn test_simple() {
        let elem: Element = "<open xmlns='urn:ietf:params:xml:ns:xmpp-framing'/>".parse().unwrap();
        let open = Open::try_from(elem).unwrap();
        assert_eq!(open.from, None);
        assert_eq!(open.to, None);
        assert_eq!(open.id, None);
        assert_eq!(open.version, None);
        assert_eq!(open.xml_lang, None);
    }
}
