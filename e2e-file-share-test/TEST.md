# E2E File Share Test

## Project

This project is a test script for using Zerobase as a backend solution in a simple toy project.

## Goals

The script should be able to use Zerobase to create a fully functional backend for the product spect described in this document.

## Product Specification

The product is a simple file sharing platform.

### Features

1. Users should be able to sign up and sign in with email/password or passkeys.
2. Users should have a name.
3. Users should be able to upload files owned by the uploader. Files have a name, a size, and a MIME type.
4. Users should be able to CRUD their files.
5. Users should be able to create directories with several files in them. A file can belong to a 0 or 1 directory.
6. Users should be able to CRUD their directories. Directories have a name.
7. Users should be able to share directories with other users.
8. A user can only see the contents of a directory if they own the directory or the owner shared the directory with them.
9. Files can only be viewed by the owner or by another user who has access to the directory containing the file.

## Test Script

The test scripts needs to set up Zerobase to support the product specification.

Zerobase code MUST NOT be modified. It needs to be used as an SDK (framework mode) or complete black box (no new code at all, only configure the collections).

Finally, the test script needs to test the product built with Zerobase. Excersize multiple scenarios of the 9 features described above.

Test script must be in rust.

