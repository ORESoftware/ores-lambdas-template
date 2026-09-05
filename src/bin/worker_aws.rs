use __CRATE__::adapters::aws::from_event;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Error> {
    run(service_fn(|event: LambdaEvent<Value>| async move { Ok::<_, Error>(serde_json::to_value(from_event(event))?) })).await
}
