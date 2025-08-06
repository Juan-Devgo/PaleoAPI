package co.juandevgo.paleoapi.infrastructure.controller;

import co.juandevgo.paleoapi.application.dto.CreateSpecieResponseDTO;
import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import jakarta.validation.Valid;
import org.springframework.http.MediaType;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;

import java.util.List;

public interface ISpeciesRestUI {

    @PostMapping(value = "species", consumes = MediaType.APPLICATION_JSON_VALUE, produces = MediaType.APPLICATION_JSON_VALUE)
    CreateSpecieResponseDTO createSpecie(@Valid @RequestBody SpecieDTO specie);

    @GetMapping(value = "species", produces = MediaType.APPLICATION_JSON_VALUE)
    List<SpecieDTO> getAllSpecies();

    @GetMapping(value = "species/name/{name}", produces = MediaType.APPLICATION_JSON_VALUE)
    List<SpecieDTO> getSpecieByName(@PathVariable("name") String name);

    @GetMapping(value = "species/group/{group}", consumes = MediaType.APPLICATION_JSON_VALUE, produces = MediaType.APPLICATION_JSON_VALUE)
    List<SpecieDTO> getSpecieByGroup(@PathVariable("group") String group);

    @GetMapping(value = "species/time-period/{timePeriod}", consumes = MediaType.APPLICATION_JSON_VALUE, produces = MediaType.APPLICATION_JSON_VALUE)
    List<SpecieDTO> getSpecieByTimePeriod(@PathVariable("timePeriod") String timePeriod);

    @GetMapping(value = "species/classification/{classification}", consumes = MediaType.APPLICATION_JSON_VALUE, produces = MediaType.APPLICATION_JSON_VALUE)
    List<SpecieDTO> getSpecieByClassification(@PathVariable("classification") String classification);

    @GetMapping(value = "species/location/{location}", consumes = MediaType.APPLICATION_JSON_VALUE, produces = MediaType.APPLICATION_JSON_VALUE)
    List<SpecieDTO> getSpecieByLocation(@PathVariable("location") String location);
}