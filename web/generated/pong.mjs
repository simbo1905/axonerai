export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("_type" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/_type"});
    else {
      if (typeof instance["_type"] !== "string" || !["pong"].includes(instance["_type"])) e.push({instancePath: "" + "/_type", schemaPath: "" + "/properties/_type" + "/enum"});
    }
    if (!("id" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/id"});
    else {
      if (instance["id"] !== null) {
        if (typeof instance["id"] !== "string") e.push({instancePath: "" + "/id", schemaPath: "" + "/properties/id" + "/type"});
      }
    }
    for (const k in instance) {
      if (k !== "_type" && k !== "id") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
