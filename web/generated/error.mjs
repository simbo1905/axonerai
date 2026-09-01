export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("_type" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/_type"});
    else {
      if (typeof instance["_type"] !== "string" || !["error"].includes(instance["_type"])) e.push({instancePath: "" + "/_type", schemaPath: "" + "/properties/_type" + "/enum"});
    }
    if (!("id" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/id"});
    else {
      if (instance["id"] !== null) {
        if (typeof instance["id"] !== "string") e.push({instancePath: "" + "/id", schemaPath: "" + "/properties/id" + "/type"});
      }
    }
    if (!("message" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/message"});
    else {
      if (typeof instance["message"] !== "string") e.push({instancePath: "" + "/message", schemaPath: "" + "/properties/message" + "/type"});
    }
    for (const k in instance) {
      if (k !== "_type" && k !== "id" && k !== "message") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
