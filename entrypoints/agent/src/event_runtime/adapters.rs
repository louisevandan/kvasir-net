//! Composition-only factories. Discovery and CREATE consume the same entries.
use p4_adapter::node_adapter::RetainedNodeAdapter;
use p4_protocol::event::Endpoint;
use std::sync::Arc;

type Factory = fn(Endpoint, usize, usize, usize, usize) -> Result<Arc<dyn RetainedNodeAdapter>, String>;

const FACTORIES: &[(&str, Factory)] = &[
    ("llamacpp", |endpoint, input, output, retained, bytes| {
        p4_llamacpp_staged_adapter::v2::RetainedLlamaNodeAdapter::new(endpoint,input,output,retained,bytes)
            .map(|adapter| Arc::new(adapter) as Arc<dyn RetainedNodeAdapter>)
            .map_err(|error|format!("invalid adapter storage: {error:?}"))
    }),
    #[cfg(feature = "hf-transformers")]
    ("hf-transformers", |endpoint, input, output, retained, bytes| {
        p4_hf_adapter::HfNodeAdapter::new(endpoint,input,output,retained,bytes)
            .map(|adapter| Arc::new(adapter) as Arc<dyn RetainedNodeAdapter>)
    }),
];

pub(super) fn kinds() -> Vec<&'static str> { FACTORIES.iter().map(|(kind,_)|*kind).collect() }

pub(super) fn create(kind:&str, endpoint:Endpoint, input:usize, output:usize, retained:usize, bytes:usize)
    -> Result<Arc<dyn RetainedNodeAdapter>,String> {
    let factory=FACTORIES.iter().find(|(name,_)| *name==kind)
        .ok_or_else(||format!("unsupported adapter kind {kind}"))?.1;
    factory(endpoint,input,output,retained,bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn advertised_factories_construct_the_real_retained_adapters() {
        let mut expected=vec!["llamacpp"];
        if cfg!(feature="hf-transformers") {expected.push("hf-transformers");}
        assert_eq!(kinds(),expected);
        for kind in expected {
            let endpoint=Endpoint::node("tcp://127.0.0.1:41990".parse().unwrap(),kind,1);
            let adapter=create(kind,endpoint,1,1,2,4096).unwrap();
            assert_eq!(adapter.completion_storage_snapshot().unwrap().retained_count,0);
            assert!(adapter.peek_retained_completion().is_none());
        }
        assert!(create("unknown",Endpoint::node("tcp://127.0.0.1:41990".parse().unwrap(),"invalid",1),1,1,2,4096).is_err());
        #[cfg(not(feature="hf-transformers"))]
        assert!(create("hf-transformers",Endpoint::node("tcp://127.0.0.1:41990".parse().unwrap(),"disabled",1),1,1,2,4096).is_err());
    }
}
