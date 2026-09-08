-- Migration 0015 has already shipped and its checksum is part of existing
-- databases, so it must remain byte-for-byte immutable. Databases that have
-- not reached 0015 yet are prepared by the safe candidate upgrader before
-- SQLx runs 0015; a later SQL migration cannot repair a conflict that would
-- make 0015 abort first.
--
-- This versioned marker records that the compatibility preflight is present.
SELECT 1;
