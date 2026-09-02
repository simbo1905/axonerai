export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("_type" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/_type"});
    else {
      if (typeof instance["_type"] !== "string" || !["tool_call"].includes(instance["_type"])) e.push({instancePath: "" + "/_type", schemaPath: "" + "/properties/_type" + "/enum"});
    }
    if (!("args_pretty" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/args_pretty"});
    else {
      if (typeof instance["args_pretty"] !== "string") e.push({instancePath: "" + "/args_pretty", schemaPath: "" + "/properties/args_pretty" + "/type"});
    }
    if (!("bytes_down" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/bytes_down"});
    else {
      if (typeof instance["bytes_down"] !== "number" || !Number.isInteger(instance["bytes_down"]) || instance["bytes_down"] < 0 || instance["bytes_down"] > 4294967295) e.push({instancePath: "" + "/bytes_down", schemaPath: "" + "/properties/bytes_down" + "/type"});
    }
    if (!("bytes_up" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/bytes_up"});
    else {
      if (typeof instance["bytes_up"] !== "number" || !Number.isInteger(instance["bytes_up"]) || instance["bytes_up"] < 0 || instance["bytes_up"] > 4294967295) e.push({instancePath: "" + "/bytes_up", schemaPath: "" + "/properties/bytes_up" + "/type"});
    }
    if (!("duration_ms" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/duration_ms"});
    else {
      if (typeof instance["duration_ms"] !== "number" || !Number.isInteger(instance["duration_ms"]) || instance["duration_ms"] < 0 || instance["duration_ms"] > 4294967295) e.push({instancePath: "" + "/duration_ms", schemaPath: "" + "/properties/duration_ms" + "/type"});
    }
    if (!("id" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/id"});
    else {
      if (instance["id"] !== null) {
        if (typeof instance["id"] !== "string") e.push({instancePath: "" + "/id", schemaPath: "" + "/properties/id" + "/type"});
      }
    }
    if (!("result_pretty" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/result_pretty"});
    else {
      if (typeof instance["result_pretty"] !== "string") e.push({instancePath: "" + "/result_pretty", schemaPath: "" + "/properties/result_pretty" + "/type"});
    }
    if (!("session_id" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/session_id"});
    else {
      if (typeof instance["session_id"] !== "string") e.push({instancePath: "" + "/session_id", schemaPath: "" + "/properties/session_id" + "/type"});
    }
    if (!("tool" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/tool"});
    else {
      if (typeof instance["tool"] !== "string") e.push({instancePath: "" + "/tool", schemaPath: "" + "/properties/tool" + "/type"});
    }
    if (!("ts" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/ts"});
    else {
      if (typeof instance["ts"] !== "number" || !Number.isFinite(instance["ts"])) e.push({instancePath: "" + "/ts", schemaPath: "" + "/properties/ts" + "/type"});
    }
    for (const k in instance) {
      if (k !== "_type" && k !== "args_pretty" && k !== "bytes_down" && k !== "bytes_up" && k !== "duration_ms" && k !== "id" && k !== "result_pretty" && k !== "session_id" && k !== "tool" && k !== "ts") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
