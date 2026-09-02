export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("_type" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/_type"});
    else {
      if (typeof instance["_type"] !== "string" || !["session_meta"].includes(instance["_type"])) e.push({instancePath: "" + "/_type", schemaPath: "" + "/properties/_type" + "/enum"});
    }
    if (!("created_at" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/created_at"});
    else {
      if (typeof instance["created_at"] !== "number" || !Number.isFinite(instance["created_at"])) e.push({instancePath: "" + "/created_at", schemaPath: "" + "/properties/created_at" + "/type"});
    }
    if (!("session_id" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/session_id"});
    else {
      if (typeof instance["session_id"] !== "string") e.push({instancePath: "" + "/session_id", schemaPath: "" + "/properties/session_id" + "/type"});
    }
    if (!("title" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/title"});
    else {
      if (typeof instance["title"] !== "string") e.push({instancePath: "" + "/title", schemaPath: "" + "/properties/title" + "/type"});
    }
    for (const k in instance) {
      if (k !== "_type" && k !== "created_at" && k !== "session_id" && k !== "title") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
