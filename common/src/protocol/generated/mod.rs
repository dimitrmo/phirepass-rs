// Auto-generated protobuf modules

// Include generated files in their own modules to avoid naming conflicts
#[allow(clippy::all)]
pub mod phirepass {
    pub mod common {
        include!("phirepass.common.rs");
    }

    pub mod web {
        include!("phirepass.web.rs");
    }

    pub mod sftp {
        include!("phirepass.sftp.rs");
    }

    // Dummy node module for WASM to satisfy frame.rs references
    #[cfg(target_arch = "wasm32")]
    pub mod node {
        // Empty placeholder struct that matches what frame.rs expects
        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct NodeFrameData {
            #[prost(oneof = "node_frame_data::Message", tags = "1")]
            pub message: ::core::option::Option<node_frame_data::Message>,
        }
        pub mod node_frame_data {
            #[derive(Clone, PartialEq, ::prost::Oneof)]
            pub enum Message {
                #[prost(message, tag = "1")]
                Dummy(()),
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub mod node {
        include!("phirepass.node.rs");
    }

    pub mod frame {
        include!("phirepass.frame.rs");
    }
}

// Re-export commonly used types at the top level for convenience
pub use phirepass::frame::{Frame, frame};
pub use phirepass::web::{WebFrameData, web_frame_data};

#[cfg(not(target_arch = "wasm32"))]
pub use phirepass::node::{NodeFrameData, node_frame_data};
