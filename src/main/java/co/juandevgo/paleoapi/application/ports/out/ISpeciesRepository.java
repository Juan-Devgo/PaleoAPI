package co.juandevgo.paleoapi.application.ports.out;

import co.juandevgo.paleoapi.domain.Specie;

import java.util.List;

public interface ISpeciesRepository {

    void createSpecie(Specie specie);

    List<Specie> getAllSpecies();

    List<Specie> findAllByName(String name);

    List<Specie> findAllByGroup(String group);

    List<Specie> findAllByTimePeriod(String timePeriod);

    List<Specie> findAllByClassification(String classification);

    List<Specie> findAllByLocation(String location);
}