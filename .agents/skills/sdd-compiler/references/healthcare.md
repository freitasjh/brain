# Healthcare & Clinic Specification Patterns

## Typical Bounded Contexts
- **Scheduling**: Appointments, Calendars, Overbooking rules.
- **Clinical (EMR/PEP)**: Electronic Medical Records, SOAP notes, Prescriptions.
- **Billing**: Health Insurance (TISS/TUSS standard in Brazil), Invoicing.

## The Real EMR (PEP) Structure
A medical record is NEVER just a simple text field. It must be structured for analytics, legal compliance, and interoperability.

**Standard EMR Structure (SOAP Format):**
- **S (Subjective)**: Patient's chief complaint and history (Text).
- **O (Objective)**: Vital signs (HR, BP, Temp), physical exam findings (Structured JSON).
- **A (Assessment)**: Diagnosis, must link to standard codes (ICD-10 / CID-10 in Brazil).
- **P (Plan)**: Medications prescribed, exams ordered, return date.

**Audit & Immutability:**
- Once a PEP is "Finalized" (signed), it becomes IMMUTABLE.
- Any subsequent changes require an explicitly tracked "Addendum" (Adendo) linked to the original record.
- Every read access to a clinical note must generate an Audit Log (LGPD compliance).

## Financial & Billing (TISS)
- In Brazil, health insurance billing requires the XML TISS standard.
- **Entities involved**: Provider (Doctor/Clinic), Beneficiary (Patient), Operator (Health Insurance).
- **Glosas (Rejections)**: Business rules must anticipate claim rejections due to missing CID-10, missing authorization passwords, or invalid patient IDs.
