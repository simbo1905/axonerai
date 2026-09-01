export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("_type" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/_type"});
    else {
      if (typeof instance["_type"] !== "string" || !["ready"].includes(instance["_type"])) e.push({instancePath: "" + "/_type", schemaPath: "" + "/properties/_type" + "/enum"});
    }
    if (!("version" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/version"});
    else {
      if (typeof instance["version"] !== "string") e.push({instancePath: "" + "/version", schemaPath: "" + "/properties/version" + "/type"});
    }
    if (!("websocket_path" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/websocket_path"});
    else {
      if (typeof instance["websocket_path"] !== "string") e.push({instancePath: "" + "/websocket_path", schemaPath: "" + "/properties/websocket_path" + "/type"});
    }
    for (const k in instance) {
      if (k !== "_type" && k !== "version" && k !== "websocket_path") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
