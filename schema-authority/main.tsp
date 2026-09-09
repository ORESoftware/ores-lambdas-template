import "@typespec/json-schema";
using TypeSpec.JsonSchema;

namespace Ores.LambdaRuntime;

@id("Provider")
enum Provider {
  awsLambda: "aws-lambda",
  gcpCloudRun: "gcp-cloud-run",
  azureFunctions: "azure-functions",
  vercel: "vercel",
  cloudflareWorkers: "cloudflare-workers",
  scintilla: "scintilla",
  local: "local",
}

@id("Operation")
enum Operation {
  health: "health",
  version: "version",
  echo: "echo",
}
