export function validate(instance) {
  const e = [];
  if (instance === null || typeof instance !== "object" || Array.isArray(instance)) {
    e.push({instancePath: "", schemaPath: "" + "/properties"});
  } else {
    if (!("_type" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/_type"});
    else {
      if (typeof instance["_type"] !== "string" || !["ack"].includes(instance["_type"])) e.push({instancePath: "" + "/_type", schemaPath: "" + "/properties/_type" + "/enum"});
    }
    if (!("for_type" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/for_type"});
    else {
      if (typeof instance["for_type"] !== "string") e.push({instancePath: "" + "/for_type", schemaPath: "" + "/properties/for_type" + "/type"});
    }
    if (!("message" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/message"});
    else {
      if (instance["message"] !== null) {
        if (typeof instance["message"] !== "string") e.push({instancePath: "" + "/message", schemaPath: "" + "/properties/message" + "/type"});
      }
    }
    if (!("ok" in instance)) e.push({instancePath: "", schemaPath: "" + "/properties/ok"});
    else {
      if (typeof instance["ok"] !== "boolean") e.push({instancePath: "" + "/ok", schemaPath: "" + "/properties/ok" + "/type"});
    }
    for (const k in instance) {
      if (k !== "_type" && k !== "for_type" && k !== "message" && k !== "ok") e.push({instancePath: "" + "/" + k, schemaPath: ""});
    }
  }
  return e;
}
