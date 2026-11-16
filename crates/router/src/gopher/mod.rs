mod serve {
    use tokio::{
        net::TcpListener,
        select, spawn,
        sync::broadcast::{Receiver, error::RecvError},
        task::{JoinHandle, JoinSet},
    };

    use crate::Router;

    /// Handle gopher protocol
    pub async fn serve(mut shutdown_rx: Receiver<()>, router: Router) -> JoinHandle<()> {
        let listener = TcpListener::bind("127.0.0.1:8020").await.unwrap();
        spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                select! {
                    // Await shutdown signal
                    result = shutdown_rx.recv() => {
                        // Close no matter the result
                        match result {
                            Ok(()) | Err(RecvError::Closed) => break,
                            Err(RecvError::Lagged(count)) => {
                                // Print error if lagged
                                tracing::warn!(
                                    count,
                                    "shutdown receiver lagged"
                                );
                                break;
                            }
                        }
                    }
                    // Await accepting client
                    result = listener.accept() => {
                        let (stream, _addr) = result.unwrap();
                        let router = router.clone();
                        connections.spawn(async move {
                            if let Err(error) = super::handle(router, stream).await {
                                println!("Gopher protocol error: {}", error);
                            }
                        });
                    }
                }
            }

            // Stop accepting new connections, then wait for existing ones.
            while let Some(result) = connections.join_next().await {
                if let Err(error) = result {
                    tracing::error!(%error, "connection task failed");
                }
            }
        })
    }
}

mod handle {
    use tokio::{
        io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
        net::TcpStream,
    };

    use super::Selector;
    use crate::{Request, Response, Router, ServerError, Uri, Version};

    const MAX_REQUEST_LINE: usize = 1024;

    pub async fn handle(router: Router, stream: TcpStream) -> Result<(), ServerError> {
        // 1. decode stream into request
        let (reader, writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let req = read_request(&mut reader).await?;

        // 2. call router.handle(request) -> response
        let res = router.handle(req).await;

        // 3. encode response into stream
        send_response(writer, res).await?;

        Ok(())
    }

    async fn read_request<R>(reader: &mut R) -> Result<Request, ServerError>
    where
        R: AsyncBufRead + Unpin,
    {
        let selector = read_line(reader).await?;
        Ok(Request {
            version: Version::Gopher,
            method: ::http::Method::GET,
            uri: Uri::Gopher(selector),
            headers: ::http::HeaderMap::new(),
            extensions: ::http::Extensions::new(),
            body: Vec::new(),
        })
    }

    async fn read_line<R>(reader: &mut R) -> Result<Selector, ServerError>
    where
        R: AsyncBufRead + Unpin,
    {
        // Get first line
        let mut first_line = String::new();
        reader.read_line(&mut first_line).await?;

        // Uri too long error
        if first_line.len() > MAX_REQUEST_LINE {
            return Err(ServerError::InvalidUri(
                "Request uri is too long".to_string(),
            ));
        }

        // Remove CRLF
        let first_line = first_line.trim_end_matches(['\r', '\n']);

        match Selector::try_from(first_line) {
            Ok(url) => Ok(url),
            _ => {
                return Err(ServerError::InvalidUri("Invalid Gemini URL".to_string()));
            }
        }
    }

    async fn send_response<W>(mut writer: W, response: Response) -> Result<(), ServerError>
    where
        W: AsyncWrite + Unpin,
    {
        writer
            .write_all(format!("{} {}\r\n", response.status, response.meta).as_bytes())
            .await?;
        writer.write_all(&response.body).await?;

        // Gemini clients use EOF to determine the end of the response.
        writer.flush().await?;

        Ok(())
    }
}

mod gopher_type {
    // Automatically handles unknown goper type as GopherType::Unknown(char)
    macro_rules! gopher_types {
        ($(
            $gophertype:ident, $gopherchar:expr;
        )*) => {
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub enum GopherType {
                $($gophertype,)*
                Unknown(char),
            }

            impl From<char> for GopherType {
                fn from(value: char) -> Self {
                    match value {
                        $($gopherchar => Self::$gophertype,)*
                        _ => Self::Unknown(value),
                    }
                }
            }
            impl From<u8> for GopherType {
                fn from(value: u8) -> Self {
                    let value: char = value.into();
                    GopherType::from(value)
                }
            }

            impl Into<char> for GopherType {
                fn into(self) -> char {
                    match self {
                        $(Self::$gophertype => $gopherchar,)*
                        Self::Unknown(a) => a,
                    }
                }
            }
        };
    }

    gopher_types! {
        // Canonical Types
        TextFile, '0';
        Submenu, '1';
        CCSONameserver, '2';
        Failure, '3';
        BinHexEncodedFile, '4';
        DOSBinaryArchive, '5';
        UUEncodedFile, '6';
        FullTextSearch, '7';
        Telnet, '8';
        BinaryFile, '9';
        MirrorServer, '+';
        GIFFile, 'g';
        ImageFile, 'I';
        Telnet3270, 'T';
        // Gopher+ Types
        BitmapFile, ':';
        MovieFile, ';';
        SoundFile, '<';
        // Non-canonical Types
        DocumentFile, 'd';
        HTMLFile, 'h';
        InformationalMessage, 'i';
        ImageFile2, 'p';
        RichTextFormatFile, 'r';
        SoundFile2, 's';
        PDFFile, 'P';
        XMLFile, 'X';
    }

    impl GopherType {
        /// Returns true if type is anything other than Failure
        pub fn is_success(&self) -> bool {
            *self != Self::Failure
        }
        /// Returns true if type is Submenus, Failure, Text search, etc
        pub fn is_text(&self) -> bool {
            match self {
                Self::Submenu
                | Self::Failure
                | Self::FullTextSearch
                | Self::Telnet
                | Self::MirrorServer
                | Self::Telnet3270
                | Self::InformationalMessage => true,
                _ => false,
            }
        }
        /// Returns true if type is a file
        pub fn is_file(&self) -> bool {
            match self {
                Self::TextFile
                | Self::BinHexEncodedFile
                | Self::DOSBinaryArchive
                | Self::UUEncodedFile
                | Self::BinaryFile
                | Self::GIFFile
                | Self::ImageFile
                | Self::BitmapFile
                | Self::MovieFile
                | Self::SoundFile
                | Self::DocumentFile
                | Self::HTMLFile
                | Self::ImageFile2
                | Self::RichTextFormatFile
                | Self::SoundFile2
                | Self::PDFFile
                | Self::XMLFile => true,
                _ => false,
            }
        }
        /// Returns true if type is a text or document file
        pub fn is_document_file(&self) -> bool {
            *self == Self::TextFile
                || *self == Self::DocumentFile
                || *self == Self::RichTextFormatFile
                || *self == Self::PDFFile
        }
        /// Returns true if type is a xml-like file
        pub fn is_xml_file(&self) -> bool {
            *self == Self::HTMLFile || *self == Self::XMLFile
        }
        /// Returns true if type is an image file
        pub fn is_image_file(&self) -> bool {
            *self == Self::ImageFile || *self == Self::ImageFile2
        }
        /// Returns true if type is a video file
        pub fn is_video_file(&self) -> bool {
            *self == Self::GIFFile || *self == Self::MovieFile
        }
        /// Returns true if type is a sound file
        pub fn is_sound_file(&self) -> bool {
            *self == Self::SoundFile || *self == Self::SoundFile2
        }
    }
}

mod selector;

pub use gopher_type::GopherType;
pub use handle::handle;
pub use selector::Selector;
pub use serve::serve;
